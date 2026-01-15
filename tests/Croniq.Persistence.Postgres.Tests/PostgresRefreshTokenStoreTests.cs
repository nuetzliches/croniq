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
public sealed class PostgresRefreshTokenStoreTests : IAsyncLifetime
{
    private readonly PostgresContainerFixture _sql;
    private ServiceProvider? _provider;
    private IRefreshTokenStore? _store;

    public PostgresRefreshTokenStoreTests(PostgresContainerFixture sql)
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
        _store = _provider.GetRequiredService<IRefreshTokenStore>();
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
    public async Task Create_and_find_active_token()
    {
        var expires = DateTimeOffset.UtcNow.AddMinutes(15);
        var created = await _store!.CreateAsync(new RefreshTokenCreateRequest(
            TenantId: "tenant-auth",
            UserId: "usr-1",
            TokenHash: "hash-rt-1",
            ExpiresAtUtc: expires));

        created.TokenHash.ShouldBe("hash-rt-1");
        created.ExpiresAtUtc.ShouldBe(expires);

        var found = await _store.FindActiveByHashAsync("tenant-auth", "hash-rt-1");
        found.ShouldNotBeNull();
        found!.TokenId.ShouldBe(created.TokenId);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Revoke_marks_token_inactive()
    {
        var created = await _store!.CreateAsync(new RefreshTokenCreateRequest(
            TenantId: "tenant-auth",
            UserId: "usr-2",
            TokenHash: "hash-rt-2",
            ExpiresAtUtc: DateTimeOffset.UtcNow.AddMinutes(10)));

        await _store.RevokeAsync("tenant-auth", created.TokenId, "rt_replacement");

        var found = await _store.FindActiveByHashAsync("tenant-auth", "hash-rt-2");
        found.ShouldBeNull();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Revoke_all_for_user_revokes_every_active_token()
    {
        await _store!.CreateAsync(new RefreshTokenCreateRequest(
            TenantId: "tenant-auth",
            UserId: "usr-3",
            TokenHash: "hash-rt-3a",
            ExpiresAtUtc: DateTimeOffset.UtcNow.AddMinutes(10)));

        await _store.CreateAsync(new RefreshTokenCreateRequest(
            TenantId: "tenant-auth",
            UserId: "usr-3",
            TokenHash: "hash-rt-3b",
            ExpiresAtUtc: DateTimeOffset.UtcNow.AddMinutes(10)));

        await _store.CreateAsync(new RefreshTokenCreateRequest(
            TenantId: "tenant-auth",
            UserId: "usr-other",
            TokenHash: "hash-rt-3c",
            ExpiresAtUtc: DateTimeOffset.UtcNow.AddMinutes(10)));

        await _store.RevokeAllForUserAsync("tenant-auth", "usr-3");

        (await _store.FindActiveByHashAsync("tenant-auth", "hash-rt-3a")).ShouldBeNull();
        (await _store.FindActiveByHashAsync("tenant-auth", "hash-rt-3b")).ShouldBeNull();
        (await _store.FindActiveByHashAsync("tenant-auth", "hash-rt-3c")).ShouldNotBeNull();
    }
}


