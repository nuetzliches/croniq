using System;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class InMemoryTenantStoreTests
{
    [Fact]
    public async Task Create_upserts_by_reference()
    {
        var store = new InMemoryTenantStore();

        var first = await store.CreateAsync("acme", "Acme");
        first.Reference.ShouldBe("acme");
        first.IsActive.ShouldBeTrue();

        var second = await store.CreateAsync("acme", "Acme Updated");
        second.TenantId.ShouldBe(first.TenantId);
        second.Name.ShouldBe("Acme Updated");

        var listed = await store.ListAsync();
        listed.ShouldContain(tenant => string.Equals(tenant.Reference, "acme", StringComparison.OrdinalIgnoreCase));
    }

    [Fact]
    public async Task Deactivate_marks_tenant_inactive()
    {
        var store = new InMemoryTenantStore();
        var tenant = await store.CreateAsync("beta", "Beta");

        var deactivated = await store.DeactivateAsync(tenant.TenantId);
        deactivated.ShouldBeTrue();

        var fetched = await store.GetByIdAsync(tenant.TenantId);
        fetched.ShouldNotBeNull();
        fetched!.IsActive.ShouldBeFalse();

        var missing = await store.DeactivateAsync("tn_missing");
        missing.ShouldBeFalse();
    }
}
