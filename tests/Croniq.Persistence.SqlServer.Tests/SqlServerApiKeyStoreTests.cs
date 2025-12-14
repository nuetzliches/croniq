using System;
using System.Threading.Tasks;
using Croniq.Auth.Abstractions;
using Croniq.Auth.SqlServer;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceWebhooks)]
public sealed class SqlServerApiKeyStoreTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IApiKeyStore? _store;

    public SqlServerApiKeyStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqAuthSqlServer(options => options.ConnectionString = _sql.ConnectionString);
        _provider = services.BuildServiceProvider();
        _store = _provider.GetRequiredService<IApiKeyStore>();
    }

    public async Task DisposeAsync()
    {
        if (_provider is IAsyncDisposable asyncDisposable)
        {
            await asyncDisposable.DisposeAsync();
        }
        else
        {
            _provider?.Dispose();
        }
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Issue_and_validate_roundtrip_succeeds()
    {
        var issued = await _store!.IssueAsync(new ApiKeyIssueRequest("tenant-sql", "client-abc", "dev", new[] { "schedules:read" }, null));

        var validation = await _store.ValidateAsync(issued.PlaintextSecret);

        validation.IsValid.ShouldBeTrue();
        validation.TenantId.ShouldBe("tenant-sql");
        validation.EnvironmentTag.ShouldBe("dev");
        validation.CallerId.ShouldBe(issued.KeyId);
        validation.Scopes.ShouldContain("schedules:read");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Revoke_marks_key_inactive()
    {
        var issued = await _store!.IssueAsync(new ApiKeyIssueRequest("tenant-sql", "client-revoke", null, Array.Empty<string>(), null));

        var revoked = await _store.RevokeAsync("tenant-sql", issued.KeyId);
        revoked.ShouldBeTrue();

        var validation = await _store.ValidateAsync(issued.PlaintextSecret);
        validation.IsValid.ShouldBeFalse();
        validation.Failure.ShouldBe("revoked");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Rotate_revokes_old_and_returns_new_secret()
    {
        var issued = await _store!.IssueAsync(new ApiKeyIssueRequest("tenant-sql", "client-rotate", "qa", new[] { "x" }, TimeSpan.FromMinutes(10)));

        var rotated = await _store.RotateAsync("tenant-sql", issued.KeyId);
        rotated.ShouldNotBeNull();

        var oldValidation = await _store.ValidateAsync(issued.PlaintextSecret);
        oldValidation.IsValid.ShouldBeFalse();
        oldValidation.Failure.ShouldBe("revoked");

        var newValidation = await _store.ValidateAsync(rotated!.PlaintextSecret);
        newValidation.IsValid.ShouldBeTrue();
        newValidation.TenantId.ShouldBe("tenant-sql");
        newValidation.EnvironmentTag.ShouldBe("qa");
        newValidation.CallerId.ShouldBe(rotated.KeyId);
        newValidation.Scopes.ShouldContain("x");
    }
}
