using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookCapabilitiesTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public WebhookCapabilitiesTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task CapabilitiesEndpointReturnsDefaultsFromConfiguration()
    {
        _host.Reset();

        var response = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/webhooks/capabilities?environment={WebhookApiTestHost.Environment}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<WebhookCapabilitiesResponse>();
        payload.ShouldNotBeNull();
        payload!.AllowUnsignedHooks.ShouldBeTrue();
        payload.DefaultRequestsPerMinute.ShouldBe(120);
    }
}
