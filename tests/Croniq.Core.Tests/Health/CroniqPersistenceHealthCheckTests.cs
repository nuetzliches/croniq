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

public sealed class CroniqPersistenceHealthCheckTests
{
    [Fact]
    public async Task Uses_persistence_health_probe_when_available()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var health = Substitute.For<IPersistenceHealth>();
        health.CheckAsync(Arg.Any<CancellationToken>())
            .Returns(Task.FromResult(new PersistenceHealthResult(true, "ok")));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });

        var check = new CroniqPersistenceHealthCheck(store, options, health);

        var result = await check.CheckHealthAsync(new HealthCheckContext(), CancellationToken.None);

        result.Status.ShouldBe(HealthStatus.Healthy);
        result.Description.ShouldBe("persistence reachable");
        result.Data.ContainsKey("provider").ShouldBeTrue();
        result.Data["detail"].ShouldBe("ok");
    }

    [Fact]
    public async Task Reports_unhealthy_when_probe_returns_unhealthy()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var health = Substitute.For<IPersistenceHealth>();
        health.CheckAsync(Arg.Any<CancellationToken>())
            .Returns(Task.FromResult(new PersistenceHealthResult(false, "down")));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });

        var check = new CroniqPersistenceHealthCheck(store, options, health);

        var result = await check.CheckHealthAsync(new HealthCheckContext(), CancellationToken.None);

        result.Status.ShouldBe(HealthStatus.Unhealthy);
        result.Description.ShouldBe("persistence unreachable");
        result.Data.ContainsKey("provider").ShouldBeTrue();
        result.Data["detail"].ShouldBe("down");
    }

    [Fact]
    public async Task Falls_back_to_listing_triggers_without_probe()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(Array.Empty<TriggerDefinition>()));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });

        var check = new CroniqPersistenceHealthCheck(store, options);

        var result = await check.CheckHealthAsync(new HealthCheckContext(), CancellationToken.None);

        result.Status.ShouldBe(HealthStatus.Healthy);
        await store.Received(1).ListTriggersAsync(
            Arg.Is<PartitionScope>(scope => scope.TenantId == "t" && scope.EnvironmentTag == "dev"),
            Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Reports_unhealthy_when_probe_throws()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var health = Substitute.For<IPersistenceHealth>();
        health.CheckAsync(Arg.Any<CancellationToken>())
            .Returns(Task.FromException<PersistenceHealthResult>(new InvalidOperationException("probe failed")));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });

        var check = new CroniqPersistenceHealthCheck(store, options, health);

        var result = await check.CheckHealthAsync(new HealthCheckContext(), CancellationToken.None);

        result.Status.ShouldBe(HealthStatus.Unhealthy);
        result.Description.ShouldBe("persistence unreachable");
        result.Data.ContainsKey("provider").ShouldBeTrue();
    }
}
