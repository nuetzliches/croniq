using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class ApiKeyAdminIntegrationTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public ApiKeyAdminIntegrationTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task IssuingApiKeyReturnsSecretAndClientMetadata()
    {
        _host.Reset();

        var request = new IssueApiKeyRequest(
            ClientId: "deploy-agent",
            EnvironmentTag: WebhookApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.WebhooksRead, CroniqScopes.WebhooksWrite },
            TtlHours: 6);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys", request);
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var issued = await response.Content.ReadFromJsonAsync<IssueApiKeyResponse>();
        issued.ShouldNotBeNull();
        issued!.PlaintextSecret.ShouldContain(issued.KeyId);
        issued.EnvironmentTag.ShouldBe(WebhookApiTestHost.Environment);

        var clientResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-clients/{request.ClientId}?environment={WebhookApiTestHost.Environment}");
        clientResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var client = await clientResponse.Content.ReadFromJsonAsync<ApiClientResponse>();
        client.ShouldNotBeNull();
        client!.ClientId.ShouldBe(request.ClientId);
        client.EnvironmentTag.ShouldBe(WebhookApiTestHost.Environment);
        client.Scopes.ShouldContain(CroniqScopes.WebhooksRead);
        client.Scopes.ShouldContain(CroniqScopes.WebhooksWrite);
    }

    [Fact]
    public async Task RotateApiKeyReturnsFreshSecret()
    {
        _host.Reset();
        var issued = await IssueKeyAsync();

        var rotateResponse = await _host.Client.PostAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/api-keys/{issued.KeyId}/rotate?environment={WebhookApiTestHost.Environment}",
            JsonContent.Create(new { }));

        rotateResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var rotated = await rotateResponse.Content.ReadFromJsonAsync<IssueApiKeyResponse>();
        rotated.ShouldNotBeNull();
        rotated!.KeyId.ShouldNotBe(issued.KeyId);
        rotated.PlaintextSecret.ShouldNotBe(issued.PlaintextSecret);
        rotated.EnvironmentTag.ShouldBe(WebhookApiTestHost.Environment);
    }

    [Fact]
    public async Task RevokingApiKeyIsIdempotent()
    {
        _host.Reset();
        var issued = await IssueKeyAsync(clientId: "revoker");

        var firstDelete = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys/{issued.KeyId}?environment={WebhookApiTestHost.Environment}");
        firstDelete.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var secondDelete = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys/{issued.KeyId}?environment={WebhookApiTestHost.Environment}");
        secondDelete.StatusCode.ShouldBe(HttpStatusCode.NoContent);
    }

    [Fact]
    public async Task ApiKeyEndpointsRequireManageScope()
    {
        _host.Reset();
        const string limitedKey = "ak_limited";
        var limitedContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "limited-client",
            Scopes: new[] { CroniqScopes.WebhooksRead });
        _host.CallerFactory.AddContext(limitedKey, limitedContext);

        SetCallerApiKey(limitedKey);
        try
        {
            var response = await _host.Client.PostAsJsonAsync(
                $"/tenants/{WebhookApiTestHost.TenantId}/api-keys",
                new IssueApiKeyRequest("ops-client", WebhookApiTestHost.Environment, null, null));

            response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
        }
        finally
        {
            SetCallerApiKey(TestCallerContextFactory.ApiKey);
        }
    }

    [Fact]
    public async Task ApiClientCrudRoundtripSucceeds()
    {
        _host.Reset();

        var upsert = new UpsertApiClientRequest(
            ClientId: "ops-agent",
            Name: "Ops Agent",
            EnvironmentTag: WebhookApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.WebhooksRead },
            IsActive: true);

        var upsertResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/api-clients?environment={WebhookApiTestHost.Environment}",
            upsert);
        upsertResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var created = await upsertResponse.Content.ReadFromJsonAsync<ApiClientResponse>();
        created.ShouldNotBeNull();
        created!.ClientId.ShouldBe(upsert.ClientId);
        created.Name.ShouldBe(upsert.Name);
        created.EnvironmentTag.ShouldBe(WebhookApiTestHost.Environment);
        created.Scopes.ShouldContain(CroniqScopes.WebhooksRead);

        var listResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-clients?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var list = await listResponse.Content.ReadFromJsonAsync<ApiClientResponse[]>();
        list.ShouldNotBeNull();
        list!.Any(c => c.ClientId == upsert.ClientId).ShouldBeTrue();
    }

    [Fact]
    public async Task DeletingApiClientRevokesMetadata()
    {
        _host.Reset();

        var upsert = new UpsertApiClientRequest(
            ClientId: "temp-client",
            Name: null,
            EnvironmentTag: WebhookApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.WebhooksRead },
            IsActive: true);

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/api-clients?environment={WebhookApiTestHost.Environment}",
            upsert);
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var delete = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-clients/{upsert.ClientId}?environment={WebhookApiTestHost.Environment}");
        delete.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var fetch = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-clients/{upsert.ClientId}?environment={WebhookApiTestHost.Environment}");
        fetch.StatusCode.ShouldBe(HttpStatusCode.NotFound);
    }

    [Fact]
    public async Task CanIssueCroniqTokens()
    {
        _host.Reset();

        var upsert = new UpsertApiClientRequest(
            ClientId: "token-client",
            Name: "Token Client",
            EnvironmentTag: WebhookApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.WebhooksRead, CroniqScopes.WebhooksWrite },
            IsActive: true);
        var upsertResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/api-clients?environment={WebhookApiTestHost.Environment}",
            upsert);
        upsertResponse.EnsureSuccessStatusCode();

        var tokenRequest = new IssueTokenRequest(
            ClientId: null,
            Scopes: new[] { CroniqScopes.WebhooksRead },
            Audience: "cronqi-api",
            TtlMinutes: 10);

        var tokenResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/api-clients/{upsert.ClientId}/tokens?environment={WebhookApiTestHost.Environment}",
            tokenRequest);

        tokenResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var token = await tokenResponse.Content.ReadFromJsonAsync<IssueTokenResponse>();
        token.ShouldNotBeNull();
        token!.TokenType.ShouldBe("Bearer");
        token.ExpiresIn.ShouldBeGreaterThan(0);
        token.AccessToken.ShouldNotBeNullOrWhiteSpace();
    }

    [Fact]
    public async Task MeEndpointReturnsCallerMetadata()
    {
        _host.Reset();

        var response = await _host.Client.GetAsync("/me");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<CallerInfoResponse>();
        payload.ShouldNotBeNull();
        payload!.TenantId.ShouldBe(WebhookApiTestHost.TenantId);
        payload.EnvironmentTag.ShouldBe(WebhookApiTestHost.Environment);
        payload.CallerType.ShouldBe(CallerType.ApiKey);
    }

    private async Task<IssueApiKeyResponse> IssueKeyAsync(string clientId = "deploy-agent")
    {
        var request = new IssueApiKeyRequest(
            ClientId: clientId,
            EnvironmentTag: WebhookApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.WebhooksRead },
            TtlHours: 12);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys", request);
        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<IssueApiKeyResponse>();
        payload.ShouldNotBeNull();
        return payload!;
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
