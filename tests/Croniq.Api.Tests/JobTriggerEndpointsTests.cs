using System.Collections.Generic;
using System.Net;
using System.Net.Http.Json;
using System.Threading;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Scheduling;
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
        var jobKey = $"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:ops:delayed";
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
}
