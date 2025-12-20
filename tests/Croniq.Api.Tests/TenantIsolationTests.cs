using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class TenantIsolationTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public TenantIsolationTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task ScheduleUpsertRejectsCrossTenant()
    {
        _host.Reset();
        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = $"other-tenant:{WebhookApiTestHost.Environment}:ops:job",
            CronExpression = "0 0/5 * * * ?",
            TriggerId = null,
            StartAtUtc = null,
            EndAtUtc = null
        };

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task ScheduleUpsertRejectsEnvironmentMismatch()
    {
        _host.Reset();
        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = $"{WebhookApiTestHost.TenantId}:prod:ops:job",
            CronExpression = "0 0/5 * * * ?",
            TriggerId = null,
            StartAtUtc = null,
            EndAtUtc = null
        };

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task TriggerJobRejectsCrossTenant()
    {
        _host.Reset();
        var request = new TriggerJobRequest($"{WebhookApiTestHost.TenantId}-other:{WebhookApiTestHost.Environment}:ops:job");

        var response = await _host.Client.PostAsJsonAsync("/jobs/trigger", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task TriggerJobRequiresScope()
    {
        _host.Reset();
        const string limitedKey = "ak_no_jobs_scope";
        var limitedContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "limited-client",
            Scopes: new[] { CroniqScopes.WebhooksRead });
        _host.CallerFactory.AddContext(limitedKey, limitedContext);

        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", limitedKey);

        var request = new TriggerJobRequest($"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:ops:job");
        var response = await _host.Client.PostAsJsonAsync("/jobs/trigger", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task ExecutionLogsRejectMismatchedTenant()
    {
        _host.Reset();
        const string executionId = "exec-foreign";
        _host.ExecutionLogs.SetLog(executionId, tenantId: "other-tenant", environmentTag: WebhookApiTestHost.Environment);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/executions/{executionId}/logs");
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task ExecutionLogsRejectEnvironmentMismatch()
    {
        _host.Reset();
        const string executionId = "exec-env";
        _host.ExecutionLogs.SetLog(executionId, tenantId: WebhookApiTestHost.TenantId, environmentTag: "prod");

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/executions/{executionId}/logs");
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task ExecutionLogsStreamForMatchingTenant()
    {
        _host.Reset();
        const string executionId = "exec-ok";
        _host.ExecutionLogs.SetLog(executionId, tenantId: WebhookApiTestHost.TenantId, environmentTag: WebhookApiTestHost.Environment);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/executions/{executionId}/logs");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        response.Content.Headers.ContentType?.MediaType.ShouldBe("application/x-ndjson");

        var body = await response.Content.ReadAsStringAsync();
        body.ShouldContain(executionId);
    }
}
