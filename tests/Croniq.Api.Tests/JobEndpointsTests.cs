using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
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
        payload.IsActive.ShouldBeTrue();
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

    [Fact]
    public async Task ActivateJobUpdatesStatus()
    {
        _host.Reset();
        var jobKey = "ops:pending";
        await UpsertJobAsync(jobKey, description: "pending job", isActive: false, assignedRunnerId: "runner-1");

        var activateResponse = await _host.Client.PostAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/jobs/{Uri.EscapeDataString(jobKey)}/activate?environment={WebhookApiTestHost.Environment}",
            content: null);
        activateResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await activateResponse.Content.ReadFromJsonAsync<JobResponse>();
        payload.ShouldNotBeNull();
        payload.JobKey.ShouldBe(jobKey);
        payload.IsActive.ShouldBeTrue();
        payload.AssignedRunnerId.ShouldBe("runner-1");
    }

    [Fact]
    public async Task ActivateJobRequiresAssignment()
    {
        _host.Reset();
        var jobKey = "ops:pending-unassigned";
        await UpsertJobAsync(jobKey, description: "pending job", isActive: false, assignedRunnerId: null);

        var activateResponse = await _host.Client.PostAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/jobs/{Uri.EscapeDataString(jobKey)}/activate?environment={WebhookApiTestHost.Environment}",
            content: null);
        activateResponse.StatusCode.ShouldBe(HttpStatusCode.BadRequest);
    }

    [Fact]
    public async Task RunnerRegistrationReassignsActiveJobWhenAssignedRunnerOffline()
    {
        _host.Reset();
        var jobKey = "ops:runner-reassign";
        await _host.JobStore.UpsertJobAsync(BuildRunnerAssignedJob(jobKey, "runner-a"), _host.DefaultScope, default);

        var runnerKey = "ak_runner_b";
        _host.CallerFactory.AddContext(
            runnerKey,
            BuildRunnerContext("runner-b", CroniqScopes.JobsRegister));
        UseApiKey(runnerKey);

        var request = new RunnerJobRegistrationRequest(
            WebhookApiTestHost.Environment,
            "runner-b",
            "instance-b",
            jobKey,
            "registered by runner",
            Metadata: null);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs:register", request);
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<JobResponse>();
        payload.ShouldNotBeNull();
        payload.AssignedRunnerId.ShouldBe("runner-b");
        payload.AssignmentSource.ShouldBe("runner");
    }

    [Fact]
    public async Task RunnerRegistrationRejectsWhenAssignedRunnerOnline()
    {
        _host.Reset();
        var jobKey = "ops:runner-conflict";
        await _host.JobStore.UpsertJobAsync(BuildRunnerAssignedJob(jobKey, "runner-a"), _host.DefaultScope, default);

        var runnerAKey = "ak_runner_a";
        _host.CallerFactory.AddContext(
            runnerAKey,
            BuildRunnerContext("runner-a", CroniqScopes.RunnersHeartbeat));
        UseApiKey(runnerAKey);

        var heartbeat = new RunnerHeartbeatRequest(
            WebhookApiTestHost.Environment,
            "runner-a",
            "instance-a",
            DateTimeOffset.UtcNow);
        var heartbeatResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/runners/heartbeat",
            heartbeat);
        heartbeatResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var runnerBKey = "ak_runner_b";
        _host.CallerFactory.AddContext(
            runnerBKey,
            BuildRunnerContext("runner-b", CroniqScopes.JobsRegister));
        UseApiKey(runnerBKey);

        var request = new RunnerJobRegistrationRequest(
            WebhookApiTestHost.Environment,
            "runner-b",
            "instance-b",
            jobKey,
            "registered by runner",
            Metadata: null);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/jobs:register", request);
        response.StatusCode.ShouldBe(HttpStatusCode.Conflict);
    }

    private async Task UpsertJobAsync(
        string jobKey,
        string? description = null,
        IDictionary<string, string>? metadata = null,
        bool? isActive = null,
        string? assignedRunnerId = "itest-client")
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
            Metadata: metadata,
            IsActive: isActive,
            AssignedRunnerId: assignedRunnerId);

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

    private void UseApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }

    private static CallerContext BuildRunnerContext(string runnerId, params string[] scopes)
    {
        return new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            runnerId,
            scopes);
    }

    private static JobDefinition BuildRunnerAssignedJob(string jobKey, string runnerId)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new InvalidOperationException($"JobKey '{jobKey}' is invalid for the test setup.");
        }

        return new JobDefinition(
            jobKey,
            parsed.NamespaceSegment,
            parsed.JobName,
            parsed.Variant,
            Description: "runner job",
            Metadata: null,
            IsActive: true,
            AssignedRunnerId: runnerId,
            AssignedBy: runnerId,
            AssignedAtUtc: DateTimeOffset.UtcNow,
            AssignmentSource: "runner");
    }
}
