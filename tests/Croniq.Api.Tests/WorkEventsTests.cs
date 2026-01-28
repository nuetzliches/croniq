using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WorkEventsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public WorkEventsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task Events_AppendsExecutionLogs()
    {
        _host.Reset();
        const string jobKey = "ops:work-events";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1));

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 1);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);
        var lease = pollPayload.Leases[0];

        var eventsRequest = new WorkEventsRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: lease,
            Events: new[]
            {
                new WorkEventEntry("hello from worker", Level: "Information")
            });

        var eventsResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/work/{lease.ExecutionId}:events",
            eventsRequest);
        eventsResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var lines = await WaitForLinesAsync(lease.ExecutionId, line => line.Contains("\"type\":\"log\""));
        lines.ShouldNotBeEmpty();
    }

    [Fact]
    public async Task Events_CanBeRetried()
    {
        _host.Reset();
        const string jobKey = "ops:work-events-retry";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1));

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 1);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);
        var lease = pollPayload.Leases[0];

        var eventsRequest = new WorkEventsRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: lease,
            Events: new[]
            {
                new WorkEventEntry("retry me", Level: "Information")
            });

        var first = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/work/{lease.ExecutionId}:events",
            eventsRequest);
        first.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var second = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/work/{lease.ExecutionId}:events",
            eventsRequest);
        second.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var lines = await WaitForLogCountAsync(lease.ExecutionId, expectedCount: 2);
        var logCount = lines.Count(line => line.Contains("\"type\":\"log\""));
        logCount.ShouldBeGreaterThanOrEqualTo(2);
    }

    [Fact]
    public async Task Events_WithRunnerMismatch_ReturnsForbidden()
    {
        _host.Reset();
        const string jobKey = "ops:work-events-mismatch";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1));

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 1);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);
        var lease = pollPayload.Leases[0];

        var eventsRequest = new WorkEventsRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-1",
            Lease: lease,
            Events: new[]
            {
                new WorkEventEntry("hello from worker", Level: "Information")
            });

        var eventsResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/work/{lease.ExecutionId}:events",
            eventsRequest);
        eventsResponse.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    private async Task SeedDueTriggerAsync(string jobKey, DateTimeOffset startAtUtc, string runnerId = "itest-client")
    {
        var scope = _host.DefaultScope;
        await _host.JobStore.UpsertJobAsync(
            new JobDefinition(jobKey, "ops", "work", Variant: null, Description: null, Metadata: null, AssignedRunnerId: runnerId),
            scope,
            CancellationToken.None);

        var triggerId = $"{jobKey}:once-{Guid.NewGuid():N}";
        var trigger = new TriggerDefinition(
            triggerId,
            jobKey,
            TriggerSchedule.OnceExpression,
            scope,
            StartAtUtc: startAtUtc,
            EndAtUtc: null,
            Enabled: true,
            Metadata: null,
            TimeZoneId: TimeZoneInfo.Utc.Id);

        await _host.JobStore.UpsertTriggerAsync(trigger, CancellationToken.None);
    }

    private async Task<List<string>> WaitForLinesAsync(string executionId, Func<string, bool> predicate)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var lines = await ReadLinesAsync(executionId);
            if (lines.Any(predicate))
            {
                return lines;
            }

            await Task.Delay(50);
        }

        return await ReadLinesAsync(executionId);
    }

    private async Task<List<string>> WaitForLogCountAsync(string executionId, int expectedCount)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var lines = await ReadLinesAsync(executionId);
            var count = lines.Count(line => line.Contains("\"type\":\"log\""));
            if (count >= expectedCount)
            {
                return lines;
            }

            await Task.Delay(50);
        }

        return await ReadLinesAsync(executionId);
    }

    private async Task<List<string>> ReadLinesAsync(string executionId)
    {
        var lines = new List<string>();
        await foreach (var line in _host.ExecutionLogs.ReadLinesAsync(executionId, CancellationToken.None))
        {
            lines.Add(line);
        }

        return lines;
    }
}
