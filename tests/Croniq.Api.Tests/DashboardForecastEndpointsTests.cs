using System;
using System.Linq;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Scheduling;
using Croniq.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class DashboardForecastEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public DashboardForecastEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task ForecastAggregatesUpcomingSchedules()
    {
        _host.Reset();
        var now = DateTimeOffset.UtcNow;

        await UpsertOnceScheduleAsync("ops:once-1", now.AddMinutes(1));
        await UpsertOnceScheduleAsync("ops:once-2", now.AddMinutes(10));

        var response = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/dashboard/forecast?environment={WebhookApiTestHost.Environment}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<ScheduleForecastResponse>();
        payload.ShouldNotBeNull();
        payload.BucketMinutes.ShouldBe(5);
        payload.TotalSchedules.ShouldBe(2);
        payload.ActiveSchedules.ShouldBe(2);

        var summary5 = payload.Summaries.Single(summary => summary.WindowMinutes == 5);
        summary5.Count.ShouldBe(1);
        var summary15 = payload.Summaries.Single(summary => summary.WindowMinutes == 15);
        summary15.Count.ShouldBe(2);
        var summary60 = payload.Summaries.Single(summary => summary.WindowMinutes == 60);
        summary60.Count.ShouldBe(2);
    }

    [Fact]
    public async Task ForecastRejectsInvalidBucketSize()
    {
        _host.Reset();

        var response = await _host.Client.GetAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/dashboard/forecast?environment={WebhookApiTestHost.Environment}&windowMinutes=60&bucketMinutes=7");

        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
    }

    private async Task UpsertOnceScheduleAsync(string jobKey, DateTimeOffset startAtUtc)
    {
        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = jobKey,
            CronExpression = TriggerSchedule.OnceExpression,
            TriggerId = $"{jobKey}:once",
            StartAtUtc = startAtUtc,
            Enabled = true
        };

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}",
            request);

        response.StatusCode.ShouldBe(HttpStatusCode.Created);
    }
}
