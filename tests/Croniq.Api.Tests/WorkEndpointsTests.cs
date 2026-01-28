using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WorkEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public WorkEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task PollThenAck_Succeeds_And_RemovesTrigger()
    {
        _host.Reset();
        const string jobKey = "ops:work";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1));

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 10);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);
        pollPayload.Leases[0].JobKey.ShouldBe(jobKey);
        pollPayload.Leases[0].ExecutionId.ShouldNotBeNullOrWhiteSpace();

        var ack = new WorkAckRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: pollPayload.Leases[0],
            Succeeded: true);

        var ackResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", ack);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var triggers = await _host.JobStore.ListTriggersAsync(_host.DefaultScope, CancellationToken.None);
        triggers.ShouldBeEmpty();

        var pollAgain = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollAgain.StatusCode.ShouldBe(HttpStatusCode.OK);
        var pollAgainPayload = await pollAgain.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollAgainPayload.ShouldNotBeNull();
        pollAgainPayload.Leases.ShouldBeEmpty();
    }

    [Fact]
    public async Task Poll_WithRunnerInstanceCollision_ReturnsConflict()
    {
        _host.Reset();

        var first = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            RunnerInstanceId: "instance-1",
            BatchSize: 1);

        var firstResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", first);
        firstResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var second = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            RunnerInstanceId: "instance-2",
            BatchSize: 1);

        var secondResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", second);
        secondResponse.StatusCode.ShouldBe(HttpStatusCode.Conflict);

        var payload = await secondResponse.Content.ReadFromJsonAsync<JsonDocument>();
        payload.ShouldNotBeNull();
        payload.RootElement.GetProperty("title").GetString().ShouldBe("runner-id-in-use");
    }

    [Fact]
    public async Task Ack_DuplicateLease_IsIdempotent()
    {
        _host.Reset();
        const string jobKey = "ops:work-ack-conflict";
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

        var ack = new WorkAckRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: pollPayload.Leases[0],
            Succeeded: true);

        var firstAck = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", ack);
        firstAck.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var secondAck = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", ack);
        secondAck.StatusCode.ShouldBe(HttpStatusCode.NoContent);
    }

    [Fact]
    public async Task Poll_RespectsAllowTestExecutions_AndReturnsIntent()
    {
        _host.Reset();
        const string jobKey = "ops:work-intent";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(
            jobKey,
            startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            executionMode: ExecutionIntent.ExecutionModes.Test,
            invocationSource: ExecutionIntent.InvocationSources.Manual);

        var rejectedPoll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 1,
            AllowTestExecutions: false);

        var rejectedResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", rejectedPoll);
        rejectedResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var rejectedPayload = await rejectedResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        rejectedPayload.ShouldNotBeNull();
        rejectedPayload.Leases.ShouldBeEmpty();

        var acceptedPoll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 1,
            AllowTestExecutions: true);

        var acceptedResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", acceptedPoll);
        acceptedResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var acceptedPayload = await acceptedResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        acceptedPayload.ShouldNotBeNull();
        acceptedPayload.Leases.Length.ShouldBe(1);
        acceptedPayload.Leases[0].ExecutionMode.ShouldBe(ExecutionIntent.ExecutionModes.Test);
        acceptedPayload.Leases[0].InvocationSource.ShouldBe(ExecutionIntent.InvocationSources.Manual);
    }

    [Fact]
    public async Task Ack_TestRejection_StoresWarningLog()
    {
        _host.Reset();
        const string jobKey = "ops:work-reject-test";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(
            jobKey,
            startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            executionMode: ExecutionIntent.ExecutionModes.Test,
            invocationSource: ExecutionIntent.InvocationSources.Manual);

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: 1,
            AllowTestExecutions: true);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);

        var ack = new WorkAckRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: pollPayload.Leases[0],
            Succeeded: false,
            DeadLetterReason: WorkRejectionReasons.TestNotAllowed);

        var ackResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", ack);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var lines = await WaitForLogLinesAsync(pollPayload.Leases[0].ExecutionId);
        lines.ShouldNotBeEmpty();

        var hasWarning = false;
        foreach (var line in lines)
        {
            using var doc = JsonDocument.Parse(line);
            if (doc.RootElement.TryGetProperty("properties", out var properties)
                && properties.TryGetProperty("croniq.warning.type", out var warningType)
                && warningType.GetString() == WorkRejectionReasons.TestNotAllowed)
            {
                hasWarning = true;
                break;
            }
        }

        hasWarning.ShouldBeTrue();
    }

    [Fact]
    public async Task Poll_UsesMaxInflight_WhenBatchSizeMissing()
    {
        _host.Reset();
        const string jobKey = "ops:work-max-inflight";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1));
        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1));

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            BatchSize: null,
            MaxInflight: 2);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<WorkPollResponse>();
        payload.ShouldNotBeNull();
        payload.Leases.Length.ShouldBe(2);
    }

    [Fact]
    public async Task Renew_Succeeds_ForActiveLease()
    {
        _host.Reset();
        const string jobKey = "ops:work-renew";
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
        var renew = new WorkRenewRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: lease);

        var renewResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/renew", renew);
        renewResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var renewPayload = await renewResponse.Content.ReadFromJsonAsync<WorkRenewResponse>();
        renewPayload.ShouldNotBeNull();
        renewPayload.Renewed.ShouldBeTrue();
        renewPayload.Lease.ShouldNotBeNull();
        renewPayload.Lease!.LeaseId.ShouldBe(lease.LeaseId);
        (renewPayload.Lease!.LeaseExpiresAtUtc >= lease.LeaseExpiresAtUtc).ShouldBeTrue();
    }

    [Fact]
    public async Task Ack_WithNextFireTime_ReschedulesTrigger()
    {
        _host.Reset();
        const string jobKey = "ops:work-retry";
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

        var retryAt = DateTimeOffset.UtcNow.AddMinutes(3);
        var ack = new WorkAckRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "itest-client",
            Lease: pollPayload.Leases[0],
            Succeeded: false,
            NextFireTimeUtc: retryAt,
            DeadLetterReason: "retry");

        var ackResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", ack);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var triggers = await _host.JobStore.ListTriggersAsync(_host.DefaultScope, CancellationToken.None);
        triggers.Count.ShouldBe(1);
        var trigger = triggers.Single();
        trigger.TriggerId.ShouldBe(pollPayload.Leases[0].TriggerId);
        trigger.StartAtUtc.ShouldNotBeNull();
        var startAt = trigger.StartAtUtc.GetValueOrDefault();
        startAt.ShouldBeInRange(retryAt.AddSeconds(-1), retryAt.AddSeconds(1));
    }

    [Fact]
    public async Task Poll_DoesNotAssignActiveLeaseToAnotherRunner()
    {
        _host.Reset();
        const string jobKey = "ops:work-concurrent";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1), runnerId: "runner-1");

        const string runnerOneKey = "ak_runner_one_claim";
        const string runnerTwoKey = "ak_runner_two_claim";
        var runnerOneContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "runner-1",
            Scopes: new[] { CroniqScopes.WorkPoll });
        var runnerTwoContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "runner-2",
            Scopes: new[] { CroniqScopes.WorkPoll });
        _host.CallerFactory.AddContext(runnerOneKey, runnerOneContext);
        _host.CallerFactory.AddContext(runnerTwoKey, runnerTwoContext);

        SetCallerApiKey(runnerOneKey);
        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-1",
            BatchSize: 1);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);

        SetCallerApiKey(runnerTwoKey);
        var secondPoll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-2",
            BatchSize: 1);

        var secondResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", secondPoll);
        secondResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var secondPayload = await secondResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        secondPayload.ShouldNotBeNull();
        secondPayload.Leases.ShouldBeEmpty();
    }

    [Fact]
    public async Task Ack_WithWrongRunner_ReturnsConflict()
    {
        _host.Reset();
        const string jobKey = "ops:work";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1), runnerId: "runner-2");

        const string runnerOneKey = "ak_runner_one";
        const string runnerTwoKey = "ak_runner_two";
        var runnerOneContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "runner-1",
            Scopes: new[] { CroniqScopes.WorkPoll, CroniqScopes.WorkAck });
        var runnerTwoContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "runner-2",
            Scopes: new[] { CroniqScopes.WorkPoll, CroniqScopes.WorkAck });
        _host.CallerFactory.AddContext(runnerOneKey, runnerOneContext);
        _host.CallerFactory.AddContext(runnerTwoKey, runnerTwoContext);

        SetCallerApiKey(runnerTwoKey);
        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-2",
            BatchSize: 1);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);
        pollPayload.Leases[0].ExecutionId.ShouldNotBeNullOrWhiteSpace();

        var wrongAck = new WorkAckRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-1",
            Lease: pollPayload.Leases[0],
            Succeeded: true);

        SetCallerApiKey(runnerOneKey);
        var wrongAckResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", wrongAck);
        wrongAckResponse.StatusCode.ShouldBe(HttpStatusCode.Conflict);

        var ack = new WorkAckRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-2",
            Lease: pollPayload.Leases[0],
            Succeeded: true);

        SetCallerApiKey(runnerTwoKey);
        var ackResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/ack", ack);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);
    }

    [Fact]
    public async Task Poll_WithRunnerMismatch_ReturnsForbidden()
    {
        _host.Reset();

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-1",
            BatchSize: 1);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task Renew_WithoutScope_ReturnsForbidden()
    {
        _host.Reset();
        const string jobKey = "ops:work-renew-scope";
        _host.EnsureJob(jobKey);

        await SeedDueTriggerAsync(jobKey, startAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1), runnerId: "runner-1");

        const string pollKey = "ak_runner_poll";
        const string renewKey = "ak_runner_no_renew";
        var pollContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "runner-1",
            Scopes: new[] { CroniqScopes.WorkPoll, CroniqScopes.WorkRenew });
        var renewContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "runner-1",
            Scopes: new[] { CroniqScopes.WorkPoll });
        _host.CallerFactory.AddContext(pollKey, pollContext);
        _host.CallerFactory.AddContext(renewKey, renewContext);

        SetCallerApiKey(pollKey);
        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-1",
            BatchSize: 1);

        var pollResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.Length.ShouldBe(1);

        SetCallerApiKey(renewKey);
        var renew = new WorkRenewRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "runner-1",
            Lease: pollPayload.Leases[0]);

        var renewResponse = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/renew", renew);
        renewResponse.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task Poll_WithoutEnvironmentAndCallerEnvironment_ReturnsBadRequest()
    {
        _host.Reset();

        const string tenantOnlyKey = "ak_tenant_only_work";
        var tenantOnlyContext = new CallerContext(
            WebhookApiTestHost.TenantId,
            EnvironmentTag: null,
            CallerType.ApiKey,
            CallerId: "tenant-only-client",
            Scopes: new[] { CroniqScopes.WorkPoll });
        _host.CallerFactory.AddContext(tenantOnlyKey, tenantOnlyContext);
        SetCallerApiKey(tenantOnlyKey);

        var poll = new WorkPollRequest(
            EnvironmentTag: null,
            RunnerId: "tenant-only-client",
            BatchSize: 1);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        response.StatusCode.ShouldBe(HttpStatusCode.BadRequest);

        var payload = await response.Content.ReadFromJsonAsync<System.Collections.Generic.Dictionary<string, string>>();
        payload.ShouldNotBeNull();
        payload["error"].ShouldBe("missing-environment");
    }

    [Fact]
    public async Task Poll_WithoutScope_ReturnsForbidden()
    {
        _host.Reset();

        const string key = "ak_missing_work_scope";
        var context = new CallerContext(
            WebhookApiTestHost.TenantId,
            WebhookApiTestHost.Environment,
            CallerType.ApiKey,
            CallerId: "no-work-scope",
            Scopes: new[] { CroniqScopes.SchedulesWrite });
        _host.CallerFactory.AddContext(key, context);
        SetCallerApiKey(key);

        var poll = new WorkPollRequest(
            EnvironmentTag: WebhookApiTestHost.Environment,
            RunnerId: "no-work-scope",
            BatchSize: 1);

        var response = await _host.Client.PostAsJsonAsync($"/tenants/{WebhookApiTestHost.TenantId}/work/poll", poll);
        response.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    private async Task SeedDueTriggerAsync(
        string jobKey,
        DateTimeOffset startAtUtc,
        string? executionMode = null,
        string? invocationSource = null,
        string runnerId = "itest-client")
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
            TimeZoneId: TimeZoneInfo.Utc.Id,
            ExecutionMode: executionMode ?? ExecutionIntent.ExecutionModes.Normal,
            InvocationSource: invocationSource ?? ExecutionIntent.InvocationSources.Schedule);

        await _host.JobStore.UpsertTriggerAsync(trigger, CancellationToken.None);
    }

    private void SetCallerApiKey(string apiKey)
    {
        _host.Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
        _host.Client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
    }

    private async Task<IReadOnlyCollection<string>> WaitForLogLinesAsync(string executionId)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var lines = await ReadLogLinesAsync(executionId);
            if (lines.Count > 0)
            {
                return lines;
            }

            await Task.Delay(50);
        }

        return await ReadLogLinesAsync(executionId);
    }

    private async Task<List<string>> ReadLogLinesAsync(string executionId)
    {
        var lines = new List<string>();
        await foreach (var line in _host.ExecutionLogs.ReadLinesAsync(executionId, CancellationToken.None))
        {
            lines.Add(line);
        }

        return lines;
    }
}
