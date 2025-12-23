using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Jobs;
using Croniq.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class JobEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public JobEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task ListJobsReturnsPersistedEntries()
    {
        _host.Reset();
        var jobKey = "ops:list";
        await UpsertJobAsync(jobKey, description: "list job");

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<JobResponse[]>();
        payload.ShouldNotBeNull();
        payload.ShouldHaveSingleItem();
        payload[0].JobKey.ShouldBe(jobKey);
        payload[0].Description.ShouldBe("list job");
    }

    [Fact]
    public async Task GetJobReturnsSingleEntry()
    {
        _host.Reset();
        var jobKey = "ops:get";
        var metadata = new Dictionary<string, string> { ["owner"] = "ops" };
        await UpsertJobAsync(jobKey, description: "get job", metadata: metadata);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs/{Uri.EscapeDataString(jobKey)}?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<JobResponse>();
        payload.ShouldNotBeNull();
        payload.JobKey.ShouldBe(jobKey);
        payload.Metadata.ShouldNotBeNull();
        payload.Metadata!["owner"].ShouldBe("ops");
    }

    [Fact]
    public async Task DeleteJobRemovesAssociatedSchedules()
    {
        _host.Reset();
        var jobKey = "ops:delete";
        await UpsertJobAsync(jobKey);
        await UpsertScheduleAsync(jobKey, "t-delete");

        var deleteResponse = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs/{Uri.EscapeDataString(jobKey)}?environment={WebhookApiTestHost.Environment}");
        deleteResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var jobs = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs?environment={WebhookApiTestHost.Environment}");
        var jobPayload = await jobs.Content.ReadFromJsonAsync<JobResponse[]>();
        jobPayload.ShouldNotBeNull();
        jobPayload.ShouldBeEmpty();

        var schedules = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}");
        var schedulePayload = await schedules.Content.ReadFromJsonAsync<ScheduleResponse[]>();
        schedulePayload.ShouldNotBeNull();
        schedulePayload.ShouldBeEmpty();
    }

    private async Task UpsertJobAsync(string jobKey, string? description = null, IDictionary<string, string>? metadata = null)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new InvalidOperationException($"JobKey '{jobKey}' is invalid for the test setup.");
        }

        var request = new UpsertJobRequest(
            JobKey: jobKey,
            Namespace: parsed.NamespaceSegment,
            Name: parsed.JobName,
            Variant: parsed.Variant,
            Description: description,
            Metadata: metadata);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs?environment={WebhookApiTestHost.Environment}", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Created);
    }

    private async Task UpsertScheduleAsync(string jobKey, string triggerSuffix)
    {
        var triggerId = $"{jobKey}:{triggerSuffix}";
        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = jobKey,
            CronExpression = "0 */5 * * * ?",
            TriggerId = triggerId,
            StartAtUtc = null,
            EndAtUtc = null,
            Enabled = true,
            Description = null,
            Metadata = null
        };

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Created);
    }
}
