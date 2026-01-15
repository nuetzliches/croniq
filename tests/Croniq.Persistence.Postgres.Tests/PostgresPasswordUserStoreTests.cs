using System;
using System.Threading.Tasks;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Postgres;
using Croniq.Persistence.Postgres.Tests.Collections;
using Croniq.TestKit.Postgres;
using Croniq.TestKit.Testing;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.Postgres.Tests;

[Collection(PostgresContractTestCollection.Name)]
[Trait(TestTraits.Component, "Auth.Postgres")]
public sealed class PostgresPasswordUserStoreTests : IAsyncLifetime
{
    private readonly PostgresContainerFixture _sql;
    private ServiceProvider? _provider;
    private IPasswordUserStore? _store;

    public PostgresPasswordUserStoreTests(PostgresContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-auth");
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqAuthPostgres(options => options.ConnectionString = _sql.ConnectionString);
        _provider = services.BuildServiceProvider();
        _store = _provider.GetRequiredService<IPasswordUserStore>();
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
    public async Task Upsert_and_find_roundtrip_succeeds()
    {
        var request = new PasswordUserUpsertRequest(
            TenantId: "tenant-auth",
            Username: "Alice",
            PasswordHash: "hash-1",
            Scopes: new[] { "schedules:read", "jobs:read" },
            IsActive: true,
            PasswordChangeRequired: true);

        var created = await _store!.UpsertAsync(request);

        created.TenantId.ShouldBe("tenant-auth");
        created.Username.ShouldBe("Alice");
        created.PasswordHash.ShouldBe("hash-1");
        created.Scopes.ShouldContain("schedules:read");
        created.PasswordChangeRequired.ShouldBeTrue();

        var byUsername = await _store.FindByUsernameAsync("tenant-auth", "alice");
        byUsername.ShouldNotBeNull();
        byUsername!.UserId.ShouldBe(created.UserId);

        var byId = await _store.FindByIdAsync("tenant-auth", created.UserId);
        byId.ShouldNotBeNull();
        byId!.Username.ShouldBe("Alice");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Records_login_failures_and_resets_on_success()
    {
        var created = await _store!.UpsertAsync(new PasswordUserUpsertRequest(
            TenantId: "tenant-auth",
            Username: "bob",
            PasswordHash: "hash-2",
            Scopes: Array.Empty<string>()));

        var lockoutEnd = DateTimeOffset.UtcNow.AddMinutes(5);
        await _store.RecordLoginFailureAsync(created.TenantId, created.UserId, lockoutEnd);

        var afterFailure = await _store.FindByIdAsync(created.TenantId, created.UserId);
        afterFailure.ShouldNotBeNull();
        afterFailure!.FailedLoginCount.ShouldBe(1);
        afterFailure.LockoutEndUtc.ShouldNotBeNull();
        afterFailure.LockoutEndUtc!.Value.UtcDateTime.ShouldBe(lockoutEnd.UtcDateTime);

        await _store.RecordLoginSuccessAsync(created.TenantId, created.UserId);

        var afterSuccess = await _store.FindByIdAsync(created.TenantId, created.UserId);
        afterSuccess.ShouldNotBeNull();
        afterSuccess!.FailedLoginCount.ShouldBe(0);
        afterSuccess.LockoutEndUtc.ShouldBeNull();
    }
}


