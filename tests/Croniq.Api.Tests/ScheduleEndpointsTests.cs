using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
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
        const string jobKey = "ops:list";
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
    public async Task ListSchedulesWithoutEnvironmentUsesCallerEnvironment()
    {
        _host.Reset();

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<ScheduleResponse[]>();
        payload.ShouldNotBeNull();
        payload.ShouldBeEmpty();
    }

    [Fact]
    public async Task ListSchedulesWithoutEnvironmentAndCallerEnvironmentReturnsBadRequest()
    {
        _host.Reset();

        const string tenantOnlyKey = "ak_tenant_only";
        var tenantOnlyContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            EnvironmentTag: null,
            CallerType.ApiKey,
            CallerId: "tenant-only-client",
            Scopes: new[] { CroniqScopes.SchedulesWrite });
        _host.CallerFactory.AddContext(tenantOnlyKey, tenantOnlyContext);
        SetCallerApiKey(tenantOnlyKey);

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules");
        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);

        var payload = await response.Content.ReadFromJsonAsync<Dictionary<string, string>>();
        payload.ShouldNotBeNull();
        payload["error"].ShouldBe("missing-environment");
    }

    [Fact]
    public async Task GetScheduleReturnsSingleEntry()
    {
        _host.Reset();
        const string jobKey = "ops:get";
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
        const string jobKey = "ops:delete";
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
        const string jobKey = "ops:env";
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

    [Fact]
    public async Task UpsertScheduleRejectsMissingCalendar()
    {
        _host.Reset();

        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = "ops:calendar-missing",
            CronExpression = "0 */5 * * * ?",
            CalendarId = "missing"
        };

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}",
            request);

        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
        var payload = await response.Content.ReadFromJsonAsync<Dictionary<string, string>>();
        payload.ShouldNotBeNull();
        payload["error"].ShouldBe("calendar-not-found");
    }

    [Fact]
    public async Task UpsertScheduleReturnsCalendarId()
    {
        _host.Reset();
        await CreateCalendarAsync("cal-schedule");

        var request = new CroniqTriggerSeedDefinition
        {
            JobKey = "ops:calendar",
            CronExpression = "0 */5 * * * ?",
            CalendarId = "cal-schedule"
        };

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}",
            request);

        response.StatusCode.ShouldBe(HttpStatusCode.Created);
        var upsert = await response.Content.ReadFromJsonAsync<ScheduleUpsertResult>();
        upsert.ShouldNotBeNull();
        upsert.CalendarId.ShouldBe("cal-schedule");

        var list = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules?environment={WebhookApiTestHost.Environment}");
        list.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await list.Content.ReadFromJsonAsync<ScheduleResponse[]>();
        payload.ShouldNotBeNull();
        payload.Length.ShouldBe(1);
        payload[0].CalendarId.ShouldBe("cal-schedule");
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

    private async Task CreateCalendarAsync(string calendarId)
    {
        var request = new CroniqCalendarSeedDefinition
        {
            CalendarId = calendarId,
            Name = "Schedule Calendar",
            Description = null,
            TimeZoneId = "UTC",
            Mode = CalendarMode.Include,
            Enabled = true,
            Rules = new List<CalendarRuleDefinition>
            {
                new(
                    "daily-window",
                    CalendarRuleType.DailyWindow,
                    SortOrder: 0,
                    IsEnabled: true,
                    DailyWindow: new CalendarDailyWindowRule("09:00", "17:00"))
            }
        };

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/calendars?environment={WebhookApiTestHost.Environment}",
            request);
        response.StatusCode.ShouldBe(HttpStatusCode.Created);
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }
}
