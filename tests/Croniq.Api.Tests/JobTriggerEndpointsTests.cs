using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using System.Threading;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class JobTriggerEndpointsTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public JobTriggerEndpointsTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task TriggerJob_WithDelay_SchedulesOnceTrigger()
    {
        _host.Reset();
        const string jobKey = "ops:delayed";
        _host.EnsureJob(jobKey);

        var request = new TriggerJobRequest(
            jobKey,
            new Dictionary<string, string> { ["source"] = "test" },
            DelaySeconds: 30);

        var response = await _host.Client.PostAsJsonAsync("/jobs/trigger", request);

        response.StatusCode.ShouldBe(HttpStatusCode.Accepted);
        _host.Pipeline.Executions.ShouldBeEmpty();

        var triggers = await _host.JobStore.ListTriggersAsync(_host.DefaultScope, CancellationToken.None);
        var trigger = triggers.ShouldHaveSingleItem();
        trigger.JobKey.ShouldBe(jobKey);
        trigger.ScheduleExpression.ShouldBe(TriggerSchedule.OnceExpression);
        trigger.StartAtUtc.ShouldNotBeNull();
        trigger.Metadata.ShouldNotBeNull();
        trigger.Metadata!.ShouldContainKeyAndValue("source", "test");
    }

    [Fact]
    public async Task TriggerJob_Schedules_WhenPersistedJobNotInRegistry()
    {
        _host.Reset();
        const string jobKey = "ops:runner";
        JobKey.TryParse(jobKey, out var parsed).ShouldBeTrue();

        var job = new JobDefinition(
            jobKey,
            parsed.NamespaceSegment,
            parsed.JobName,
            parsed.Variant,
            Description: "runner job",
            Metadata: new Dictionary<string, string> { ["registrationSource"] = "runner" },
            IsActive: true,
            AssignedRunnerId: "runner-1",
            AssignedBy: "runner-1",
            AssignedAtUtc: DateTimeOffset.UtcNow,
            AssignmentSource: "runner");

        await _host.JobStore.UpsertJobAsync(job, _host.DefaultScope, CancellationToken.None);

        var request = new TriggerJobRequest(
            jobKey,
            new Dictionary<string, string> { ["source"] = "test" });

        var response = await _host.Client.PostAsJsonAsync("/jobs/trigger", request);

        response.StatusCode.ShouldBe(HttpStatusCode.Accepted);
        _host.Pipeline.Executions.ShouldBeEmpty();

        var triggers = await _host.JobStore.ListTriggersAsync(_host.DefaultScope, CancellationToken.None);
        var trigger = triggers.ShouldHaveSingleItem();
        trigger.JobKey.ShouldBe(jobKey);
        trigger.ScheduleExpression.ShouldBe(TriggerSchedule.OnceExpression);
        trigger.Metadata.ShouldNotBeNull();
        trigger.Metadata!.ShouldContainKeyAndValue("source", "test");
        trigger.InvocationSource.ShouldBe(ExecutionIntent.InvocationSources.Manual);
    }
}
