using System;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WorkersEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public WorkersEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task HeartbeatThenList_ReturnsWorker()
    {
        _host.Reset();

        var heartbeat = new WorkerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            InstanceId: "itest-client",
            SeenAtUtc: DateTimeOffset.UtcNow,
            MetadataJson: "{\"kind\":\"worker\"}");

        var hbResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/workers/heartbeat",
            heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/workers?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<WorkerListResponse>();
        payload.ShouldNotBeNull();
        payload.Workers.Length.ShouldBe(1);
        payload.Workers[0].InstanceId.ShouldBe("itest-client");
        payload.Workers[0].IsOnline.ShouldBeTrue();
        payload.Workers[0].MetadataJson.ShouldBe("{\"kind\":\"worker\"}");
    }

    [Fact]
    public async Task Heartbeat_WithoutEnvironmentAndCallerEnvironment_ReturnsBadRequest()
    {
        _host.Reset();

        const string tenantOnlyKey = "ak_tenant_only_worker";
        var tenantOnlyContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            EnvironmentTag: null,
            CallerType.ApiKey,
            CallerId: "tenant-only-client",
            Scopes: new[] { CroniqScopes.WorkersHeartbeat });
        _host.CallerFactory.AddContext(tenantOnlyKey, tenantOnlyContext);
        SetCallerApiKey(tenantOnlyKey);

        var heartbeat = new WorkerHeartbeatRequest(
            EnvironmentTag: null,
            InstanceId: "tenant-only-client",
            SeenAtUtc: DateTimeOffset.UtcNow);

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/workers/heartbeat",
            heartbeat);
        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);

        var error = await response.Content.ReadFromJsonAsync<System.Collections.Generic.Dictionary<string, string>>();
        error.ShouldNotBeNull();
        error["error"].ShouldBe("missing-environment");
    }

    [Fact]
    public async Task Heartbeat_WithoutScope_ReturnsForbidden()
    {
        _host.Reset();

        const string key = "ak_missing_worker_scope";
        var context = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "no-worker-scope",
            Scopes: new[] { CroniqScopes.SchedulesWrite });
        _host.CallerFactory.AddContext(key, context);
        SetCallerApiKey(key);

        var heartbeat = new WorkerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            InstanceId: "no-worker-scope",
            SeenAtUtc: DateTimeOffset.UtcNow);

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/workers/heartbeat",
            heartbeat);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task List_PrunesExpiredHeartbeats()
    {
        _host.Reset();

        var heartbeat = new WorkerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            InstanceId: "itest-client",
            SeenAtUtc: DateTimeOffset.UtcNow.AddMinutes(-5));

        var hbResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/workers/heartbeat",
            heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/workers?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<WorkerListResponse>();
        payload.ShouldNotBeNull();
        payload.Workers.ShouldBeEmpty();
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
