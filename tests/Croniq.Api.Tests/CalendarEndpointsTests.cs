using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class CalendarEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public CalendarEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task UpsertCalendarCreatesAndListsEntry()
    {
        _host.Reset();

        var request = BuildCalendarRequest("cal-ops", "Ops Calendar");
        var upsert = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/calendars?environment={WebhookApiTestHost.Environment}",
            request);

        upsert.StatusCode.ShouldBe(HttpStatusCode.Created);
        var result = await upsert.Content.ReadFromJsonAsync<CalendarUpsertResult>();
        result.ShouldNotBeNull();
        result.CalendarId.ShouldBe("cal-ops");

        var list = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/calendars?environment={WebhookApiTestHost.Environment}");
        list.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await list.Content.ReadFromJsonAsync<CalendarResponse[]>();
        payload.ShouldNotBeNull();
        payload.Length.ShouldBe(1);
        payload[0].CalendarId.ShouldBe("cal-ops");
        payload[0].Name.ShouldBe("Ops Calendar");
        payload[0].Mode.ShouldBe(CalendarMode.Include);
    }

    [Fact]
    public async Task GetCalendarReturnsNotFoundWhenMissing()
    {
        _host.Reset();

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/calendars/missing?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.NotFound);
    }

    [Fact]
    public async Task DeleteCalendarRemovesEntry()
    {
        _host.Reset();

        var request = BuildCalendarRequest("cal-delete", "Delete Calendar");
        var upsert = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/calendars?environment={WebhookApiTestHost.Environment}",
            request);
        upsert.StatusCode.ShouldBe(HttpStatusCode.Created);

        var delete = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/calendars/cal-delete?environment={WebhookApiTestHost.Environment}");
        delete.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var list = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/calendars?environment={WebhookApiTestHost.Environment}");
        list.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await list.Content.ReadFromJsonAsync<CalendarResponse[]>();
        payload.ShouldNotBeNull();
        payload.ShouldBeEmpty();
    }

    [Fact]
    public async Task UpsertCalendarRejectsInvalidRequest()
    {
        _host.Reset();

        var request = new CroniqCalendarSeedDefinition
        {
            CalendarId = string.Empty,
            Name = string.Empty,
            TimeZoneId = "UTC",
            Mode = CalendarMode.Include
        };

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/calendars?environment={WebhookApiTestHost.Environment}",
            request);

        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
        var payload = await response.Content.ReadFromJsonAsync<Dictionary<string, string>>();
        payload.ShouldNotBeNull();
        payload["error"].ShouldBe("invalid-request");
    }

    private static CroniqCalendarSeedDefinition BuildCalendarRequest(string calendarId, string name)
    {
        return new CroniqCalendarSeedDefinition
        {
            CalendarId = calendarId,
            Name = name,
            Description = "Default calendar",
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
    }
}
