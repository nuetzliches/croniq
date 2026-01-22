using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api;
using Croniq.Api.Models;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookRemoteHealthTests
{
    private const string TenantId = "tenant-health";
    private const string EnvironmentTag = "dev";
    private const string ApiKey = "health-key";

    [Fact]
    public async Task RemoteHealth_ReturnsOk_WhenRemoteIsHealthy()
    {
        var handler = new StubHttpMessageHandler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        await using var app = CreateApp(new Dictionary<string, string?>
        {
            ["Croniq:Webhooks:Mode"] = "Remote",
            ["Croniq:Webhooks:Remote:BaseUrl"] = "http://dmz.croniq.test/api/",
            ["Croniq:Webhooks:Remote:ApiKey"] = "dmz-key"
        }, handler);

        var client = app.GetTestClient();
        client.DefaultRequestHeaders.Add("X-Croniq-Key", ApiKey);

        var response = await client.GetAsync($"/tenants/{TenantId}/webhooks/remote/health?environment={EnvironmentTag}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<WebhookRemoteHealthResponse>();
        payload.ShouldNotBeNull();
        payload!.Status.ShouldBe("ok");
        payload.StatusCode.ShouldBe((int)HttpStatusCode.OK);
        payload.CheckedAtUtc.ShouldBeGreaterThan(DateTimeOffset.UtcNow.AddMinutes(-1));
        handler.LastRequestUri.ShouldNotBeNull();
        handler.LastRequestUri!.AbsoluteUri.ShouldBe("http://dmz.croniq.test/api/health");
    }

    [Fact]
    public async Task RemoteHealth_ReturnsUnhealthy_WhenRemoteRespondsWithError()
    {
        var handler = new StubHttpMessageHandler(_ => new HttpResponseMessage(HttpStatusCode.ServiceUnavailable)
        {
            Content = new StringContent("dmz unavailable")
        });
        await using var app = CreateApp(new Dictionary<string, string?>
        {
            ["Croniq:Webhooks:Mode"] = "Remote",
            ["Croniq:Webhooks:Remote:BaseUrl"] = "http://dmz.croniq.test/api/",
            ["Croniq:Webhooks:Remote:ApiKey"] = "dmz-key"
        }, handler);

        var client = app.GetTestClient();
        client.DefaultRequestHeaders.Add("X-Croniq-Key", ApiKey);

        var response = await client.GetAsync($"/tenants/{TenantId}/webhooks/remote/health?environment={EnvironmentTag}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<WebhookRemoteHealthResponse>();
        payload.ShouldNotBeNull();
        payload!.Status.ShouldBe("unhealthy");
        payload.StatusCode.ShouldBe((int)HttpStatusCode.ServiceUnavailable);
        payload.Detail.ShouldNotBeNull();
        payload.Detail!.ShouldContain("dmz unavailable");
    }

    [Fact]
    public async Task RemoteHealth_ReturnsNotConfigured_WhenModeIsNotRemote()
    {
        var handler = new StubHttpMessageHandler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        await using var app = CreateApp(new Dictionary<string, string?>
        {
            ["Croniq:Webhooks:Mode"] = "InMemory"
        }, handler);

        var client = app.GetTestClient();
        client.DefaultRequestHeaders.Add("X-Croniq-Key", ApiKey);

        var response = await client.GetAsync($"/tenants/{TenantId}/webhooks/remote/health?environment={EnvironmentTag}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<WebhookRemoteHealthResponse>();
        payload.ShouldNotBeNull();
        payload!.Status.ShouldBe("not-configured");
        handler.CallCount.ShouldBe(0);
    }

    [Fact]
    public async Task Capabilities_ReturnsIngressBaseUrl_WhenConfigured()
    {
        var handler = new StubHttpMessageHandler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        await using var app = CreateApp(new Dictionary<string, string?>
        {
            ["Croniq:Webhooks:Mode"] = "Remote",
            ["Croniq:Webhooks:Remote:BaseUrl"] = "http://dmz-admin.croniq.test",
            ["Croniq:Webhooks:Remote:IngressBaseUrl"] = "http://dmz-webhooks.croniq.test",
            ["Croniq:Webhooks:Remote:ApiKey"] = "dmz-key"
        }, handler);

        var client = app.GetTestClient();
        client.DefaultRequestHeaders.Add("X-Croniq-Key", ApiKey);

        var response = await client.GetAsync($"/tenants/{TenantId}/webhooks/capabilities?environment={EnvironmentTag}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<WebhookCapabilitiesResponse>();
        payload.ShouldNotBeNull();
        payload!.RemoteBaseUrl.ShouldBe("http://dmz-admin.croniq.test");
        payload.RemoteIngressBaseUrl.ShouldBe("http://dmz-webhooks.croniq.test");
    }

    [Fact]
    public async Task Capabilities_FallsBackToBaseUrl_WhenIngressBaseUrlMissing()
    {
        var handler = new StubHttpMessageHandler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        await using var app = CreateApp(new Dictionary<string, string?>
        {
            ["Croniq:Webhooks:Mode"] = "Remote",
            ["Croniq:Webhooks:Remote:BaseUrl"] = "http://dmz-admin.croniq.test",
            ["Croniq:Webhooks:Remote:ApiKey"] = "dmz-key"
        }, handler);

        var client = app.GetTestClient();
        client.DefaultRequestHeaders.Add("X-Croniq-Key", ApiKey);

        var response = await client.GetAsync($"/tenants/{TenantId}/webhooks/capabilities?environment={EnvironmentTag}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<WebhookCapabilitiesResponse>();
        payload.ShouldNotBeNull();
        payload!.RemoteIngressBaseUrl.ShouldBe("http://dmz-admin.croniq.test");
    }

    private static WebApplication CreateApp(
        IReadOnlyDictionary<string, string?> overrides,
        StubHttpMessageHandler handler)
    {
        var builder = WebApplication.CreateBuilder();
        builder.WebHost.UseTestServer();

        var config = new Dictionary<string, string?>
        {
            ["Croniq:Api:RequestsPerMinute"] = "0",
            ["Croniq:Auth:Mode"] = "InMemory",
            ["Croniq:Auth:InMemory:ApiKey"] = ApiKey,
            ["Croniq:Auth:InMemory:TenantId"] = TenantId,
            ["Croniq:Auth:InMemory:EnvironmentTag"] = EnvironmentTag
        };
        foreach (var entry in overrides)
        {
            config[entry.Key] = entry.Value;
        }

        builder.Configuration.AddInMemoryCollection(config);

        builder.Services.AddCroniqApiServices(builder.Configuration);
        builder.Services.AddCroniqApiRateLimiter();
        builder.Services.AddSingleton<IHttpClientFactory>(new StubHttpClientFactory(handler));

        var app = builder.Build();
        app.UseCroniqApi();
        app.StartAsync().GetAwaiter().GetResult();
        return app;
    }

    private sealed class StubHttpClientFactory : IHttpClientFactory
    {
        private readonly HttpMessageHandler _handler;

        public StubHttpClientFactory(HttpMessageHandler handler)
        {
            _handler = handler ?? throw new ArgumentNullException(nameof(handler));
        }

        public HttpClient CreateClient(string name)
        {
            return new HttpClient(_handler, disposeHandler: false);
        }
    }

    private sealed class StubHttpMessageHandler : HttpMessageHandler
    {
        private readonly Func<HttpRequestMessage, HttpResponseMessage> _responseFactory;

        public StubHttpMessageHandler(Func<HttpRequestMessage, HttpResponseMessage> responseFactory)
        {
            _responseFactory = responseFactory ?? throw new ArgumentNullException(nameof(responseFactory));
        }

        public Uri? LastRequestUri { get; private set; }

        public int CallCount { get; private set; }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            CallCount++;
            LastRequestUri = request.RequestUri;
            return Task.FromResult(_responseFactory(request));
        }
    }
}
