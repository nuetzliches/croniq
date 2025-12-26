using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Hosting;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using NSubstitute;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public sealed class CroniqJobRegistrySyncHostedServiceTests
{
    private const string TestTenantId = "00000000-0000-0000-0000-000000000001";

    [Fact]
    public async Task Returns_when_sync_off_in_run_mode()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var registry = BuildRegistry();

        var service = new CroniqJobRegistrySyncHostedService(
            store,
            registry,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqJobRegistrySyncOptions { Mode = "Off" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqJobRegistrySyncHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().ListJobsAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
        await store.DidNotReceive().UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Validate_mode_does_not_write_jobs()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var registry = BuildRegistry();

        var service = new CroniqJobRegistrySyncHostedService(
            store,
            registry,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqJobRegistrySyncOptions { Mode = "CreateIfMissing" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Validate" }),
            NullLogger<CroniqJobRegistrySyncHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Syncs_missing_jobs_when_create_if_missing()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.GetJobAsync(Arg.Any<string>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<JobDefinition?>(null));
        store.UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);

        var registry = BuildRegistry();

        var service = new CroniqJobRegistrySyncHostedService(
            store,
            registry,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqJobRegistrySyncOptions { Mode = "CreateIfMissing", ManagedBy = "tests" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqJobRegistrySyncHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.Received(1).UpsertJobAsync(
            Arg.Is<JobDefinition>(job => job.JobKey == "samples:job" && job.Metadata != null && job.Metadata.ContainsKey("managedBy")),
            Arg.Is<PartitionScope>(scope => scope.TenantId == TestTenantId && scope.EnvironmentTag == "dev"),
            Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Force_update_skips_when_managedBy_missing_or_mismatch()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.ListJobsAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>()).Returns(
            Task.FromResult<IReadOnlyCollection<JobDefinition>>(new[]
            {
                new JobDefinition("samples:job", "samples", "job", null, Description: "custom", Metadata: new Dictionary<string, string> { ["managedBy"] = "someone-else" })
            }));

        var registry = BuildRegistry();

        var service = new CroniqJobRegistrySyncHostedService(
            store,
            registry,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqJobRegistrySyncOptions { Mode = "ForceUpdate", ManagedBy = "tests" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqJobRegistrySyncHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Force_update_skips_when_existing_job_has_no_managedBy_metadata()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.ListJobsAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>()).Returns(
            Task.FromResult<IReadOnlyCollection<JobDefinition>>(new[]
            {
                new JobDefinition("samples:job", "samples", "job", null, Description: "custom", Metadata: new Dictionary<string, string>())
            }));

        var registry = BuildRegistry();

        var service = new CroniqJobRegistrySyncHostedService(
            store,
            registry,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqJobRegistrySyncOptions { Mode = "ForceUpdate", ManagedBy = "tests" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqJobRegistrySyncHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Force_update_updates_when_managedBy_matches_and_preserves_description()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        store.ListJobsAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>()).Returns(
            Task.FromResult<IReadOnlyCollection<JobDefinition>>(new[]
            {
                new JobDefinition(
                    "samples:job",
                    "samples",
                    "job",
                    null,
                    Description: "custom-description",
                    Metadata: new Dictionary<string, string> { ["managedBy"] = "tests", ["custom"] = "keep" })
            }));
        store.UpsertJobAsync(Arg.Any<JobDefinition>(), Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.CompletedTask);

        var registry = BuildRegistry();

        var service = new CroniqJobRegistrySyncHostedService(
            store,
            registry,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantMode = TenantMode.Multi, TenantId = TestTenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqJobRegistrySyncOptions { Mode = "ForceUpdate", ManagedBy = "tests" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" }),
            NullLogger<CroniqJobRegistrySyncHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.Received(1).UpsertJobAsync(
            Arg.Is<JobDefinition>(job =>
                job.JobKey == "samples:job"
                && job.Description == "custom-description"
                && job.Metadata != null
                && job.Metadata.ContainsKey("managedBy") && job.Metadata["managedBy"] == "tests"
                && job.Metadata.ContainsKey("custom") && job.Metadata["custom"] == "keep"),
            Arg.Is<PartitionScope>(scope => scope.TenantId == TestTenantId && scope.EnvironmentTag == "dev"),
            Arg.Any<CancellationToken>());
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
