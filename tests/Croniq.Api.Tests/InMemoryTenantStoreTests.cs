using System;
using System.Linq;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class InMemoryTenantStoreTests
{
    [Fact]
    public async Task Create_throws_when_tenant_id_missing()
    {
        var store = new InMemoryTenantStore();

        await Should.ThrowAsync<ArgumentException>(() =>
            store.CreateAsync(new TenantCreateRequest("Acme", " ")));
    }

    [Fact]
    public async Task Create_throws_when_name_missing()
    {
        var store = new InMemoryTenantStore();

        await Should.ThrowAsync<ArgumentException>(() =>
            store.CreateAsync(new TenantCreateRequest(" ", Guid.NewGuid().ToString("D"))));
    }

    [Fact]
    public async Task Create_upserts_by_tenant_id()
    {
        var store = new InMemoryTenantStore();

        var tenantId = Guid.NewGuid().ToString("D");

        var first = await store.CreateAsync(new TenantCreateRequest("Acme", tenantId));
        first.TenantId.ShouldBe(tenantId);
        first.IsActive.ShouldBeTrue();

        var second = await store.CreateAsync(new TenantCreateRequest("Acme Updated", tenantId));
        second.TenantId.ShouldBe(first.TenantId);
        second.Name.ShouldBe("Acme Updated");

        var listed = await store.ListAsync();
        listed.ShouldContain(tenant => string.Equals(tenant.TenantId, tenantId, StringComparison.OrdinalIgnoreCase));
    }

    [Fact]
    public async Task Deactivate_marks_tenant_inactive()
    {
        var store = new InMemoryTenantStore();
        var tenant = await store.CreateAsync(new TenantCreateRequest("Beta", Guid.NewGuid().ToString("D")));

        var deactivated = await store.DeactivateAsync(tenant.TenantId);
        deactivated.ShouldBeTrue();

        var fetched = await store.GetByIdAsync(tenant.TenantId);
        fetched.ShouldNotBeNull();
        fetched!.IsActive.ShouldBeFalse();

        var missing = await store.DeactivateAsync("tn_missing");
        missing.ShouldBeFalse();
    }

    [Fact]
    public async Task GetById_returns_null_when_missing()
    {
        var store = new InMemoryTenantStore();

        (await store.GetByIdAsync("missing")).ShouldBeNull();
    }

    [Fact]
    public async Task List_orders_by_tenant_id_case_insensitive()
    {
        var store = new InMemoryTenantStore();
        var tenantA = "Alpha";
        var tenantB = "beta";
        await store.CreateAsync(new TenantCreateRequest("b", tenantB));
        await store.CreateAsync(new TenantCreateRequest("a", tenantA));

        var list = (await store.ListAsync()).ToArray();
        list.Length.ShouldBe(2);
        list[0].TenantId.ShouldBe("Alpha");
        list[1].TenantId.ShouldBe("beta");
    }
}
