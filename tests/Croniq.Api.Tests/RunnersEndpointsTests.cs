using System;
using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class RunnersEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public RunnersEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task HeartbeatThenList_ReturnsRunner()
    {
        _host.Reset();

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            SeenAtUtc: DateTimeOffset.UtcNow,
            MetadataJson: "{\"kind\":\"http\"}");

        var hbResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<RunnerListResponse>();
        payload.ShouldNotBeNull();
        payload.Runners.Length.ShouldBe(1);
        payload.Runners[0].RunnerId.ShouldBe("itest-client");
        payload.Runners[0].IsOnline.ShouldBeTrue();
        payload.Runners[0].MetadataJson.ShouldBe("{\"kind\":\"http\"}");
    }

    [Fact]
    public async Task Heartbeat_WithRunnerInstanceCollision_ReturnsConflict()
    {
        _host.Reset();

        var first = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            RunnerInstanceId: "instance-1",
            SeenAtUtc: DateTimeOffset.UtcNow,
            MetadataJson: "{\"kind\":\"http\"}");

        var firstResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat",
            first);
        firstResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var second = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            RunnerInstanceId: "instance-2",
            SeenAtUtc: DateTimeOffset.UtcNow,
            MetadataJson: "{\"kind\":\"http\"}");

        var secondResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat",
            second);
        secondResponse.StatusCode.ShouldBe(HttpStatusCode.Conflict);

        var payload = await secondResponse.Content.ReadFromJsonAsync<JsonDocument>();
        payload.ShouldNotBeNull();
        payload.RootElement.GetProperty("title").GetString().ShouldBe("runner-id-in-use");
    }

    [Fact]
    public async Task Heartbeat_WithoutEnvironmentAndCallerEnvironment_ReturnsBadRequest()
    {
        _host.Reset();

        const string tenantOnlyKey = "ak_tenant_only_runner";
        var tenantOnlyContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            EnvironmentTag: null,
            CallerType.ApiKey,
            CallerId: "tenant-only-client",
            Scopes: new[] { CroniqScopes.RunnersHeartbeat });
        _host.CallerFactory.AddContext(tenantOnlyKey, tenantOnlyContext);
        SetCallerApiKey(tenantOnlyKey);

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: null,
            RunnerId: "tenant-only-client",
            SeenAtUtc: DateTimeOffset.UtcNow);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);

        var error = await response.Content.ReadFromJsonAsync<System.Collections.Generic.Dictionary<string, string>>();
        error.ShouldNotBeNull();
        error["error"].ShouldBe("missing-environment");
    }

    [Fact]
    public async Task Heartbeat_WithoutScope_ReturnsForbidden()
    {
        _host.Reset();

        const string key = "ak_missing_runner_scope";
        var context = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "no-runner-scope",
            Scopes: new[] { CroniqScopes.SchedulesWrite });
        _host.CallerFactory.AddContext(key, context);
        SetCallerApiKey(key);

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "no-runner-scope",
            SeenAtUtc: DateTimeOffset.UtcNow);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task List_PrunesExpiredHeartbeats()
    {
        _host.Reset();

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            SeenAtUtc: DateTimeOffset.UtcNow.AddMinutes(-5));

        var hbResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<RunnerListResponse>();
        payload.ShouldNotBeNull();
        payload.Runners.ShouldBeEmpty();
    }

    [Fact]
    public async Task List_WithIncludeOffline_ReturnsOfflineRunner()
    {
        _host.Reset();

        const string offlineKey = "ak_runner_offline";
        var offlineContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "itest-offline",
            Scopes: new[] { CroniqScopes.RunnersHeartbeat, CroniqScopes.RunnersRead });
        _host.CallerFactory.AddContext(offlineKey, offlineContext);
        SetCallerApiKey(offlineKey);

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-offline",
            SeenAtUtc: DateTimeOffset.UtcNow.AddMinutes(-5));

        var hbResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners?environment={WebhookApiTestHost.Environment}&includeOffline=true");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<RunnerListResponse>();
        payload.ShouldNotBeNull();
        payload.Runners.ShouldContain(runner => runner.RunnerId == "itest-offline" && runner.IsOnline == false);
    }

    [Fact]
    public async Task DrainRunner_UpdatesMetadata()
    {
        _host.Reset();

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            SeenAtUtc: DateTimeOffset.UtcNow,
            MetadataJson: "{\"kind\":\"http\"}");

        var hbResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var drain = new RunnerDrainRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            Draining: true);

        var drainResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners/itest-client:drain",
            drain);
        drainResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<RunnerListResponse>();
        payload.ShouldNotBeNull();
        var runner = payload.Runners.ShouldHaveSingleItem();
        runner.MetadataJson.ShouldNotBeNull();

        using var doc = JsonDocument.Parse(runner.MetadataJson!);
        doc.RootElement.GetProperty("kind").GetString().ShouldBe("http");
        doc.RootElement.GetProperty("draining").GetBoolean().ShouldBeTrue();
    }

    [Fact]
    public async Task DeregisterRunner_RemovesRunner()
    {
        _host.Reset();

        var heartbeat = new RunnerHeartbeatRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            SeenAtUtc: DateTimeOffset.UtcNow,
            MetadataJson: "{\"kind\":\"http\"}");

        var hbResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat", heartbeat);
        hbResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var deleteResponse = await _host.Client.DeleteAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners/itest-client?environment={WebhookApiTestHost.Environment}");
        deleteResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var listResponse = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners?environment={WebhookApiTestHost.Environment}&includeOffline=true");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await listResponse.Content.ReadFromJsonAsync<RunnerListResponse>();
        payload.ShouldNotBeNull();
        payload.Runners.ShouldBeEmpty();
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
