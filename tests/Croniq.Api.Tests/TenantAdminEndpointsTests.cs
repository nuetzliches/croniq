using System.Linq;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class TenantAdminEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public TenantAdminEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task CreateTenantAndFetchById()
    {
        _host.Reset();

        var tenantId = $"test-{Guid.NewGuid():N}";

        var createResponse = await _host.Client.PostAsJsonAsync("/tenants", new UpsertTenantRequest(tenantId, "Acme Corp"));
        createResponse.StatusCode.ShouldBe(HttpStatusCode.Created);

        var created = await createResponse.Content.ReadFromJsonAsync<TenantResponse>();
        created.ShouldNotBeNull();
        created!.TenantId.ShouldBe(tenantId);
        created.Name.ShouldBe("Acme Corp");
        created.IsActive.ShouldBeTrue();

        var getResponse = await _host.Client.GetAsync($"/tenants/{created.TenantId}");
        getResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var fetched = await getResponse.Content.ReadFromJsonAsync<TenantResponse>();
        fetched.ShouldNotBeNull();
        fetched!.TenantId.ShouldBe(created.TenantId);
        fetched.Name.ShouldBe("Acme Corp");
    }

    [Fact]
    public async Task ListTenantsHonorsStateFilter()
    {
        _host.Reset();

        var active = await CreateTenantAsync("Active Corp");
        var inactive = await CreateTenantAsync("Inactive Corp");

        var deleteResponse = await _host.Client.DeleteAsync($"/tenants/{inactive.TenantId}");
        deleteResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var activeOnly = await _host.Client.GetFromJsonAsync<TenantResponse[]>("/tenants");
        activeOnly.ShouldNotBeNull();
        activeOnly!.Any(t => string.Equals(t.TenantId, inactive.TenantId, StringComparison.OrdinalIgnoreCase)).ShouldBeFalse();

        var allTenants = await _host.Client.GetFromJsonAsync<TenantResponse[]>("/tenants?state=all");
        allTenants.ShouldNotBeNull();
        allTenants!.Any(t => string.Equals(t.TenantId, inactive.TenantId, StringComparison.OrdinalIgnoreCase) && !t.IsActive).ShouldBeTrue();
        allTenants.Any(t => string.Equals(t.TenantId, active.TenantId, StringComparison.OrdinalIgnoreCase)).ShouldBeTrue();
    }

    [Fact]
    public async Task DeleteTenantReturnsNotFoundWhenMissing()
    {
        _host.Reset();

        var response = await _host.Client.DeleteAsync("/tenants/tn_missing");
        response.StatusCode.ShouldBe(HttpStatusCode.NotFound);
    }

    [Fact]
    public async Task TenantEndpointsRequireAdminScope()
    {
        _host.Reset();
        const string limitedKey = "ak_no_tenant_admin";
        var limitedContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "limited",
            Scopes: new[] { CroniqScopes.ApiKeysManage });
        _host.CallerFactory.AddContext(limitedKey, limitedContext);

        SetCallerApiKey(limitedKey);
        try
        {
            var response = await _host.Client.PostAsJsonAsync(
                "/tenants",
                new UpsertTenantRequest($"test-{Guid.NewGuid():N}", "Blocked"));
            response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
        }
        finally
        {
            SetCallerApiKey(TestCallerContextFactory.ApiKey);
        }
    }

    private async Task<TenantResponse> CreateTenantAsync(string name)
    {
        var response = await _host.Client.PostAsJsonAsync(
            "/tenants",
            new UpsertTenantRequest($"test-{Guid.NewGuid():N}", name));
        response.StatusCode.ShouldBe(HttpStatusCode.Created);
        var tenant = await response.Content.ReadFromJsonAsync<TenantResponse>();
        tenant.ShouldNotBeNull();
        return tenant!;
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
