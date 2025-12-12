using System;
using System.Net;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class SecurityRegressionTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public SecurityRegressionTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task UnknownApiKeyIsRejected()
    {
        _host.Reset();
        SetCallerApiKey("ak_unknown");

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);
    }

    [Fact]
    public async Task ExpiredApiKeyIsRejected()
    {
        _host.Reset();
        const string expiredKey = "ak_expired";
        var expiredContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "expired-client",
            Scopes: new[] { CroniqScopes.WebhooksRead },
            IsActive: false);
        _host.CallerFactory.AddContext(expiredKey, expiredContext);
        SetCallerApiKey(expiredKey);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);
    }

    [Fact]
    public async Task RevokedApiKeyStopsWorkingAfterDeactivation()
    {
        _host.Reset();
        const string revokableKey = "ak_revokable";
        var activeContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "revokable-client",
            Scopes: new[] { CroniqScopes.WebhooksRead },
            IsActive: true);
        _host.CallerFactory.AddContext(revokableKey, activeContext);
        SetCallerApiKey(revokableKey);

        var initial = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}");
        initial.StatusCode.ShouldBe(HttpStatusCode.OK);

        var revokedContext = activeContext with { IsActive = false };
        _host.CallerFactory.AddContext(revokableKey, revokedContext);

        var afterRevoke = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}");
        afterRevoke.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);
    }

    [Fact]
    public async Task MissingScopeReturnsForbidden()
    {
        _host.Reset();
        const string limitedKey = "ak_missing_scope";
        var limitedContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "limited-client",
            Scopes: Array.Empty<string>());
        _host.CallerFactory.AddContext(limitedKey, limitedContext);
        SetCallerApiKey(limitedKey);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
