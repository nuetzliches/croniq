using Croniq.Persistence.Abstractions;
using Croniq.Webhooks.InMemory;
using Shouldly;
using Xunit;

namespace Croniq.Webhooks.Tests;

public sealed class InMemoryWebhookDeadLetterStoreTests
{
    private static readonly PartitionScope DefaultScope = new("tenant-a", "dev");

    [Fact]
    public async Task CreateAsync_and_RecordFailureAsync_update_entries()
    {
        var store = new InMemoryWebhookDeadLetterStore();

        var id = await store.CreateAsync(
            new WebhookDeadLetterCreate(
                "hook-a",
                "job-a",
                DefaultScope.TenantId,
                DefaultScope.EnvironmentTag,
                Payload: "{}",
                Headers: null,
                Metadata: new Dictionary<string, string> { ["source"] = "unit" },
                FailureReason: "boom",
                StatusCode: 500,
                ErrorDetails: "fail",
                ExpiresAtUtc: DateTimeOffset.UtcNow.AddDays(1)),
            CancellationToken.None);

        var entry = await store.FindAsync(id, DefaultScope, CancellationToken.None);
        entry.ShouldNotBeNull();
        entry!.Attempts.ShouldBe(1);
        entry.Metadata.ShouldNotBeNull();
        entry.Metadata!.ShouldContainKeyAndValue("source", "unit");

        await store.RecordFailureAsync(
            id,
            DefaultScope,
            new WebhookDeadLetterFailure("retry", 503, "oops", DateTimeOffset.UtcNow.AddMinutes(5)),
            CancellationToken.None);

        var updated = await store.FindAsync(id, DefaultScope, CancellationToken.None);
        updated.ShouldNotBeNull();
        updated!.Attempts.ShouldBe(2);
        updated.FailureReason.ShouldBe("retry");
        updated.StatusCode.ShouldBe(503);
        updated.NextAttemptAtUtc.ShouldNotBeNull();
    }

    [Fact]
    public async Task ResolveAsync_removes_entries()
    {
        var store = new InMemoryWebhookDeadLetterStore();

        var id = await store.CreateAsync(
            new WebhookDeadLetterCreate(
                "hook-a",
                "job-a",
                DefaultScope.TenantId,
                DefaultScope.EnvironmentTag,
                Payload: "{}",
                Headers: null,
                Metadata: null,
                FailureReason: "boom",
                StatusCode: 500,
                ErrorDetails: "fail",
                ExpiresAtUtc: null),
            CancellationToken.None);

        store.Contains(id).ShouldBeTrue();
        await store.ResolveAsync(id, DefaultScope, CancellationToken.None);
        store.Contains(id).ShouldBeFalse();

        var entry = await store.FindAsync(id, DefaultScope, CancellationToken.None);
        entry.ShouldBeNull();
    }
}
