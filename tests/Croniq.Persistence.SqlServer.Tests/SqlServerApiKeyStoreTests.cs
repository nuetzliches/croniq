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
    private ITenantStore? _tenants;

    public SqlServerApiKeyStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-sql");
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqAuthSqlServer(options => options.ConnectionString = _sql.ConnectionString);
        _provider = services.BuildServiceProvider();
        _store = _provider.GetRequiredService<IApiKeyStore>();
        _tenants = _provider.GetRequiredService<ITenantStore>();
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
        validation.CallerId.ShouldBe(issued.ClientId);
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
        newValidation.CallerId.ShouldBe(rotated.ClientId);
        newValidation.Scopes.ShouldContain("x");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Upsert_and_list_clients_work()
    {
        var upsert = new ApiClientUpsertRequest(
            TenantId: "tenant-sql",
            ClientId: "ops-cli",
            Name: "Ops CLI",
            EnvironmentTag: "dev",
            Scopes: new[] { "jobs:trigger" },
            IsActive: true);

        var descriptor = await _store!.UpsertClientAsync(upsert);
        descriptor.ClientId.ShouldBe("ops-cli");
        descriptor.Scopes.ShouldContain("jobs:trigger");

        var listed = await _store.ListClientsAsync("tenant-sql", "dev");
        listed.ShouldContain(client => client.ClientId == "ops-cli");

        var fetched = await _store.GetClientAsync("tenant-sql", "ops-cli");
        fetched.ShouldNotBeNull();
        fetched!.Name.ShouldBe("Ops CLI");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Deleting_client_revokes_keys()
    {
        var issued = await _store!.IssueAsync(new ApiKeyIssueRequest("tenant-sql", "client-delete", "dev", new[] { "schedules:read" }, null));

        var deleted = await _store.DeleteClientAsync("tenant-sql", "client-delete");
        deleted.ShouldBeTrue();

        var client = await _store.GetClientAsync("tenant-sql", "client-delete");
        client.ShouldBeNull();

        var validation = await _store.ValidateAsync(issued.PlaintextSecret);
        validation.IsValid.ShouldBeFalse();
        validation.Failure.ShouldBe("revoked");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Tenant_create_list_roundtrip_succeeds()
    {
        var tenantId = Guid.NewGuid().ToString("D");
        var descriptor = await _tenants!.CreateAsync(new TenantCreateRequest("Sql Corp", tenantId));

        descriptor.TenantId.ShouldBe(tenantId);
        descriptor.IsActive.ShouldBeTrue();

        var fetched = await _tenants.GetByIdAsync(tenantId);
        fetched.ShouldNotBeNull();
        fetched!.TenantId.ShouldBe(descriptor.TenantId);

        var listed = await _tenants.ListAsync();
        listed.ShouldContain(tenant => tenant.TenantId == descriptor.TenantId);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task Tenant_deactivate_marks_row_inactive()
    {
        var tenantId = Guid.NewGuid().ToString("D");
        var descriptor = await _tenants!.CreateAsync(new TenantCreateRequest("Inactive Corp", tenantId));

        var deactivated = await _tenants.DeactivateAsync(descriptor.TenantId);
        deactivated.ShouldBeTrue();

        var fetched = await _tenants.GetByIdAsync(descriptor.TenantId);
        fetched.ShouldNotBeNull();
        fetched!.IsActive.ShouldBeFalse();

        var missing = await _tenants.DeactivateAsync("tn_missing");
        missing.ShouldBeFalse();
    }
}
