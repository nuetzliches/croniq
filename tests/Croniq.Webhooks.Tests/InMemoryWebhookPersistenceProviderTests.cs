using System.Security.Cryptography;
using System.Text;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks.InMemory;
using Shouldly;
using Xunit;

namespace Croniq.Webhooks.Tests;

public sealed class InMemoryWebhookPersistenceProviderTests
{
    private static readonly PartitionScope DefaultScope = new("tenant-a", "dev");

    [Fact]
    public void Seed_stores_definition_and_scopes_entries()
    {
        var provider = new InMemoryWebhookPersistenceProvider();

        var definition = provider.Seed(
            "hook-a",
            "job-a",
            DefaultScope,
            "secret-1",
            metadata: new Dictionary<string, string> { ["Owner"] = "platform" });

        definition.Secret.ShouldBe("secret-1");
        definition.Metadata.ShouldNotBeNull();
        definition.Metadata!.ShouldContainKeyAndValue("owner", "platform");

        provider.Find("hook-a", DefaultScope).ShouldNotBeNull();
        provider.Find("hook-a", new PartitionScope("tenant-a", "prod")).ShouldBeNull();
    }

    [Fact]
    public async Task UpsertAsync_preserves_secret_when_not_provided()
    {
        var provider = new InMemoryWebhookPersistenceProvider();

        var create = new WebhookEndpointUpsert(
            "hook-a",
            "job-a",
            DefaultScope.TenantId,
            DefaultScope.EnvironmentTag,
            Enabled: true,
            RequireSignature: true,
            RequestsPerMinute: 30,
            Secret: "secret-1",
            SignatureVersion: 1,
            Metadata: null);

        await provider.UpsertAsync(create, CancellationToken.None);
        var initial = provider.Find("hook-a", DefaultScope)!;

        var update = create with
        {
            JobKey = "job-b",
            Secret = null,
            Metadata = new Dictionary<string, string> { ["Owner"] = "ops" }
        };

        await provider.UpsertAsync(update, CancellationToken.None);
        var refreshed = provider.Find("hook-a", DefaultScope)!;

        refreshed.Secret.ShouldBe(initial.Secret);
        refreshed.JobKey.ShouldBe("job-b");
        refreshed.Metadata.ShouldNotBeNull();
        refreshed.Metadata!.ShouldContainKeyAndValue("owner", "ops");
    }

    [Fact]
    public async Task AddIpRuleAsync_sorts_and_rejects_duplicates()
    {
        var provider = new InMemoryWebhookPersistenceProvider();
        provider.Seed("hook-a", "job-a", DefaultScope, "secret-1");

        var rule1 = await provider.AddIpRuleAsync(
            new WebhookIpRuleCreate("hook-a", DefaultScope.TenantId, DefaultScope.EnvironmentTag, "10.0.0.0/24", "office", "seed", null),
            CancellationToken.None);
        var rule2 = await provider.AddIpRuleAsync(
            new WebhookIpRuleCreate("hook-a", DefaultScope.TenantId, DefaultScope.EnvironmentTag, "0.0.0.0/0", "all", "seed", null),
            CancellationToken.None);

        rule1.Id.ShouldBe(1);
        rule2.Id.ShouldBe(2);

        var rules = await provider.ListIpRulesAsync("hook-a", DefaultScope, CancellationToken.None);
        rules.Select(rule => rule.Cidr).ShouldBe(new[] { "0.0.0.0/0", "10.0.0.0/24" });

        await Should.ThrowAsync<InvalidOperationException>(() =>
            provider.AddIpRuleAsync(
                new WebhookIpRuleCreate("hook-a", DefaultScope.TenantId, DefaultScope.EnvironmentTag, "10.0.0.0/24", "dup", "seed", null),
                CancellationToken.None));
    }

    [Fact]
    public async Task RotateSecretAsync_updates_secret_and_hashes()
    {
        var provider = new InMemoryWebhookPersistenceProvider();
        provider.Seed("hook-a", "job-a", DefaultScope, "secret-1");

        var result = await provider.RotateSecretAsync(
            new WebhookSecretRotate("hook-a", DefaultScope.TenantId, DefaultScope.EnvironmentTag, null, 60, "tester", null),
            CancellationToken.None);

        result.Secret.ShouldNotBe("secret-1");
        result.SecretHash.ShouldBe(ComputeHash(result.Secret));
        result.ExpiresAtUtc.ShouldNotBeNull();

        var secrets = await provider.GetActiveSecretsAsync("hook-a", DefaultScope, CancellationToken.None);
        var secret = secrets.ShouldHaveSingleItem();
        secret.Secret.ShouldBe(result.Secret);
        secret.SecretHash.ShouldBe(result.SecretHash);
    }

    private static string ComputeHash(string secret)
    {
        var bytes = Encoding.UTF8.GetBytes(secret);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash);
    }
}
