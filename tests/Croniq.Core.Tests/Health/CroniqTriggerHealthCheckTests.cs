using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Health;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Diagnostics.HealthChecks;
using Microsoft.Extensions.Options;
using NSubstitute;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Health;

public sealed class CroniqTriggerHealthCheckTests
{
    [Fact]
    public async Task Returns_degraded_when_required_and_no_triggers_found()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(Array.Empty<TriggerDefinition>()));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantReference = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });
        var healthOptions = Microsoft.Extensions.Options.Options.Create(new CroniqHealthCheckOptions { RequireTriggers = true });

        var check = new CroniqTriggerHealthCheck(store, options, healthOptions);

        var result = await check.CheckHealthAsync(new HealthCheckContext(), CancellationToken.None);

        result.Status.ShouldBe(HealthStatus.Degraded);
        result.Description.ShouldBe("no triggers loaded");
        result.Data["triggerCount"].ShouldBe(0);
    }

    [Fact]
    public async Task Returns_unhealthy_when_trigger_listing_fails()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromException<IReadOnlyCollection<TriggerDefinition>>(new InvalidOperationException("boom")));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantReference = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });
        var healthOptions = Microsoft.Extensions.Options.Options.Create(new CroniqHealthCheckOptions());

        var check = new CroniqTriggerHealthCheck(store, options, healthOptions);

        var result = await check.CheckHealthAsync(new HealthCheckContext(), CancellationToken.None);

        result.Status.ShouldBe(HealthStatus.Unhealthy);
    }
}
