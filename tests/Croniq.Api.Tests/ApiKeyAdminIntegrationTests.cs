using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using FluentAssertions;
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
        response.StatusCode.Should().Be(HttpStatusCode.OK);

        var issued = await response.Content.ReadFromJsonAsync<IssueApiKeyResponse>();
        issued.Should().NotBeNull();
        issued!.PlaintextSecret.Should().Contain(issued.KeyId);
        issued.EnvironmentTag.Should().Be(WebhookApiTestHost.Environment);

        var clientResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-clients/{request.ClientId}?environment={WebhookApiTestHost.Environment}");
        clientResponse.StatusCode.Should().Be(HttpStatusCode.OK);

        var client = await clientResponse.Content.ReadFromJsonAsync<ApiClientResponse>();
        client.Should().NotBeNull();
        client!.ClientId.Should().Be(request.ClientId);
        client.EnvironmentTag.Should().Be(WebhookApiTestHost.Environment);
        client.Scopes.Should().Contain(new[] { CroniqScopes.WebhooksRead, CroniqScopes.WebhooksWrite });
    }

    [Fact]
    public async Task RotateApiKeyReturnsFreshSecret()
    {
        _host.Reset();
        var issued = await IssueKeyAsync();

        var rotateResponse = await _host.Client.PostAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/api-keys/{issued.KeyId}/rotate?environment={WebhookApiTestHost.Environment}",
            JsonContent.Create(new { }));

        rotateResponse.StatusCode.Should().Be(HttpStatusCode.OK);
        var rotated = await rotateResponse.Content.ReadFromJsonAsync<IssueApiKeyResponse>();
        rotated.Should().NotBeNull();
        rotated!.KeyId.Should().NotBe(issued.KeyId);
        rotated.PlaintextSecret.Should().NotBe(issued.PlaintextSecret);
        rotated.EnvironmentTag.Should().Be(WebhookApiTestHost.Environment);
    }

    [Fact]
    public async Task RevokingApiKeyIsIdempotent()
    {
        _host.Reset();
        var issued = await IssueKeyAsync(clientId: "revoker");

        var firstDelete = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys/{issued.KeyId}?environment={WebhookApiTestHost.Environment}");
        firstDelete.StatusCode.Should().Be(HttpStatusCode.NoContent);

        var secondDelete = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys/{issued.KeyId}?environment={WebhookApiTestHost.Environment}");
        secondDelete.StatusCode.Should().Be(HttpStatusCode.NoContent);
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

            response.StatusCode.Should().Be(HttpStatusCode.Forbidden);
        }
        finally
        {
            SetCallerApiKey(TestCallerContextFactory.ApiKey);
        }
    }

    private async Task<IssueApiKeyResponse> IssueKeyAsync(string clientId = "deploy-agent")
    {
        var request = new IssueApiKeyRequest(
            ClientId: clientId,
            EnvironmentTag: WebhookApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.WebhooksRead },
            TtlHours: 12);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/api-keys", request);
        response.StatusCode.Should().Be(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<IssueApiKeyResponse>();
        payload.Should().NotBeNull();
        return payload!;
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
