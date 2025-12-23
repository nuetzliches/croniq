using System;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Execution;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class ExecutionEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public ExecutionEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task ListExecutionsFiltersByJobKey()
    {
        _host.Reset();
        var match = CreateSummary(jobKey: "ops:match", status: ExecutionStatus.Failed, startedOffsetMinutes: -1);
        var other = CreateSummary(jobKey: "ops:other", status: ExecutionStatus.Succeeded, startedOffsetMinutes: -5);
        _host.ExecutionHistory.SetExecutions(new[] { match, other });

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/executions?environment={WebhookApiTestHost.Environment}&jobKey={Uri.EscapeDataString(match.JobKey)}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<ExecutionResponse[]>();
        payload.ShouldNotBeNull();
        payload.ShouldHaveSingleItem();
        payload[0].ExecutionId.ShouldBe(match.ExecutionId);
        payload[0].Status.ShouldBe(ExecutionStatus.Failed);
    }

    [Fact]
    public async Task GetExecutionReturnsSummary()
    {
        _host.Reset();
        var summary = CreateSummary(status: ExecutionStatus.Succeeded, startedOffsetMinutes: -2);
        _host.ExecutionHistory.SetExecutions(new[] { summary });

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/executions/{summary.ExecutionId}?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<ExecutionResponse>();
        payload.ShouldNotBeNull();
        payload.ExecutionId.ShouldBe(summary.ExecutionId);
        payload.JobKey.ShouldBe(summary.JobKey);
        payload.Status.ShouldBe(ExecutionStatus.Succeeded);
    }

    private static ExecutionSummary CreateSummary(string? executionId = null, string? jobKey = null, ExecutionStatus? status = null, int startedOffsetMinutes = 0)
    {
        var startedAt = DateTimeOffset.UtcNow.AddMinutes(startedOffsetMinutes);
        var execution = executionId ?? Guid.NewGuid().ToString("N");
        var job = jobKey ?? "ops:job";
        return new ExecutionSummary(
            execution,
            ExecutionKind.Job,
            WorkflowId: null,
            job,
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            TriggerId: "trg",
            FireAtUtc: startedAt,
            StartedAtUtc: startedAt,
            CompletedAtUtc: startedAt.AddSeconds(5),
            Status: status,
            DurationMs: 5080,
            InstanceId: "node",
            TraceId: Guid.NewGuid().ToString("N"),
            CorrelationId: Guid.NewGuid().ToString("N"),
            ErrorType: status == ExecutionStatus.Failed ? "System.Exception" : null,
            ErrorMessage: status == ExecutionStatus.Failed ? "boom" : null);
    }
}
