using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Hosting;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using NSubstitute;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public sealed class CroniqTriggerSeedingHostedServiceTests
{
    private const string TestTenantId = "00000000-0000-0000-0000-000000000001";

    [Fact]
    public async Task Returns_when_seeding_off_in_run_mode()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var registry = BuildRegistry();

        var service = new CroniqTriggerSeedingHostedService(
            BuildConfiguration(),
            store,
            registry,
            System.Array.Empty<CroniqTriggerSeedRegistration>(),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqSeedingOptions { Mode = "Off" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqTriggerSeedingHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
        await store.DidNotReceive().UpsertTriggerAsync(Arg.Any<TriggerDefinition>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Validate_mode_does_not_write_triggers()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var registry = BuildRegistry();

        var service = new CroniqTriggerSeedingHostedService(
            BuildConfiguration(),
            store,
            registry,
            System.Array.Empty<CroniqTriggerSeedRegistration>(),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqSeedingOptions { Mode = "CreateIfMissing" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Validate" }),
            NullLogger<CroniqTriggerSeedingHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().UpsertTriggerAsync(Arg.Any<TriggerDefinition>(), Arg.Any<CancellationToken>());
        await store.DidNotReceive().UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Seeds_missing_jobs_and_triggers()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.GetJobAsync(Arg.Any<string>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<JobDefinition?>(null));
        store.UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);
        store.ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(System.Array.Empty<TriggerDefinition>()));
        store.UpsertTriggerAsync(Arg.Any<TriggerDefinition>(), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);

        var registry = BuildRegistry();

        var service = new CroniqTriggerSeedingHostedService(
            BuildConfiguration(),
            store,
            registry,
            System.Array.Empty<CroniqTriggerSeedRegistration>(),
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqSeedingOptions { Mode = "CreateIfMissing" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqTriggerSeedingHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.Received(1).UpsertJobAsync(
            Arg.Is<JobDefinition>(job => job.JobKey == "samples:job"),
            Arg.Is<PartitionScope>(scope => scope.TenantId == TestTenantId && scope.EnvironmentTag == "dev"),
            Arg.Any<CancellationToken>());
        await store.Received(1).UpsertTriggerAsync(
            Arg.Is<TriggerDefinition>(trigger => trigger.TriggerId == "seed-trigger"),
            Arg.Any<CancellationToken>());
    }

    private static IConfiguration BuildConfiguration()
    {
        var data = new Dictionary<string, string?>
        {
            ["Croniq:Triggers:0:TriggerId"] = "seed-trigger",
            ["Croniq:Triggers:0:JobKey"] = "samples:job",
            ["Croniq:Triggers:0:CronExpression"] = "0 * * * * ?",
            ["Croniq:Triggers:0:Enabled"] = "true",
            ["Croniq:Triggers:0:ManagedBy"] = "tests"
        };

        return new ConfigurationBuilder()
            .AddInMemoryCollection(data)
            .Build();
    }

    private static IJobRegistry BuildRegistry()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" });
        var registrations = new[] { new JobRegistration(typeof(SampleJob)) };
        return new JobRegistry(options, registrations);
    }

    [CroniqJob("samples", "job")]
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
            => Task.CompletedTask;
    }
}
