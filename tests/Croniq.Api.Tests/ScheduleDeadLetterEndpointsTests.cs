using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class ScheduleDeadLetterEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public ScheduleDeadLetterEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task List_returns_schedule_deadletters()
    {
        _host.Reset();
        const string jobKey = "ops:deadletter";
        var entry = _host.JobDeadLetters.Add(new JobDeadLetterEntry(
            Id: 0,
            TriggerId: "trigger-1",
            JobKey: jobKey,
            TenantId: WebhookApiTestHost.TenantId,
            EnvironmentTag: WebhookApiTestHost.Environment,
            FireAtUtc: DateTimeOffset.UtcNow.AddMinutes(-2),
            Reason: "boom",
            Payload: "payload",
            Metadata: new Dictionary<string, string> { ["initiator"] = "test" },
            CreatedAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            ExpiresAtUtc: null));

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/schedules/deadletters?environment={WebhookApiTestHost.Environment}");

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<ScheduleDeadLetterResponse[]>();
        payload.ShouldNotBeNull();
        var record = payload!.ShouldHaveSingleItem();
        record.Id.ShouldBe(entry.Id);
        record.Metadata.ShouldNotBeNull();
        record.Metadata!.ShouldContainKeyAndValue("initiator", "test");
    }

    [Fact]
    public async Task Replay_executes_job_and_resolves_entry()
    {
        _host.Reset();
        const string jobKey = "ops:replay";
        _host.EnsureJob(jobKey);

        var entry = _host.JobDeadLetters.Add(new JobDeadLetterEntry(
            Id: 0,
            TriggerId: "trigger-9",
            JobKey: jobKey,
            TenantId: WebhookApiTestHost.TenantId,
            EnvironmentTag: WebhookApiTestHost.Environment,
            FireAtUtc: DateTimeOffset.UtcNow.AddMinutes(-2),
            Reason: "boom",
            Payload: "payload",
            Metadata: new Dictionary<string, string> { ["initiator"] = "test" },
            CreatedAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            ExpiresAtUtc: null));

        var response = await _host.Client.PostAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/schedules/deadletters/{entry.Id}/replay?environment={WebhookApiTestHost.Environment}",
            content: null);

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        _host.Pipeline.Executions.ShouldHaveSingleItem();
        var execution = _host.Pipeline.Executions[0];
        execution.JobKey.Value.ShouldBe(jobKey);
        execution.Metadata.ShouldContainKeyAndValue("initiator", "test");
        execution.Metadata.ShouldContainKeyAndValue("trigger_id", entry.TriggerId);
        execution.Metadata.ShouldContainKey("deadletter:id");

        var remaining = await _host.JobDeadLetters.ListAsync(_host.DefaultScope, default);
        remaining.ShouldBeEmpty();
    }
}
