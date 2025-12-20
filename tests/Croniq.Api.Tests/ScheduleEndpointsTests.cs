using System;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class ScheduleEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public ScheduleEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task ListSchedulesReturnsPersistedEntries()
    {
        _host.Reset();
        var jobKey = $"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:ops:list";
        var triggerId = await UpsertScheduleAsync(jobKey);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<ScheduleResponse[]>();
        payload.ShouldNotBeNull();
        payload.Length.ShouldBe(1);
        payload[0].TriggerId.ShouldBe(triggerId);
        payload[0].JobKey.ShouldBe(jobKey);
        payload[0].TenantId.ShouldBe(WebhookApiTestHost.TenantId);
        payload[0].EnvironmentTag.ShouldBe(WebhookApiTestHost.Environment);
    }

    [Fact]
    public async Task GetScheduleReturnsSingleEntry()
    {
        _host.Reset();
        var jobKey = $"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:ops:get";
        var triggerId = await UpsertScheduleAsync(jobKey);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules/{Uri.EscapeDataString(triggerId)}?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<ScheduleResponse>();
        payload.ShouldNotBeNull();
        payload.TriggerId.ShouldBe(triggerId);
        payload.JobKey.ShouldBe(jobKey);
    }

    [Fact]
    public async Task DeleteScheduleRemovesTrigger()
    {
        _host.Reset();
        var jobKey = $"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:ops:delete";
        var triggerId = await UpsertScheduleAsync(jobKey);

        var deleteResponse = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules/{Uri.EscapeDataString(triggerId)}?environment={WebhookApiTestHost.Environment}");
        deleteResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var list = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}");
        list.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await list.Content.ReadFromJsonAsync<ScheduleResponse[]>();
        payload.ShouldNotBeNull();
        payload.ShouldBeEmpty();
    }

    [Fact]
    public async Task UpsertScheduleRejectsEnvironmentQueryMismatch()
    {
        _host.Reset();
        var jobKey = $"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:ops:env";
        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = jobKey,
            CronExpression = "0 */5 * * * ?",
            TriggerId = null,
            StartAtUtc = null,
            EndAtUtc = null
        };

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment=prod",
            request);

        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    private async Task<string> UpsertScheduleAsync(string jobKey, string? triggerId = null)
    {
        var identifier = triggerId ?? $"{jobKey}:manual";
        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = jobKey,
            CronExpression = "0 */5 * * * ?",
            TriggerId = identifier,
            StartAtUtc = null,
            EndAtUtc = null,
            Enabled = true,
            Description = null,
            Metadata = null
        };

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Created);
        return identifier;
    }
}
