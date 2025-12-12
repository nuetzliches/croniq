using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests;

public class ServiceCollectionExtensionsTests
{
    [Fact]
    public void Registers_core_services_and_jobs()
    {
        var services = new ServiceCollection();

        services.AddCroniqCore(options =>
        {
            options.TenantId = "t";
            options.EnvironmentTag = "dev";
        });
        services.AddLogging();
        services.AddSingleton<IJobStore, StubJobStore>();
        services.AddCroniqJob<SampleJob>();

        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<TriggerWorker>().ShouldNotBeNull();
        provider.GetRequiredService<IJobExecutionPipeline>().ShouldBeOfType<DefaultJobExecutionPipeline>();
        provider.GetRequiredService<IMisfirePolicy>().ShouldBeOfType<DefaultMisfirePolicy>();
        provider.GetRequiredService<IJobRegistry>().TryGet(JobKey.Create("t", "dev", "core", "sample"), out _).ShouldBeTrue();
    }

    [Fact]
    public void AddCroniqJob_throws_when_attribute_missing()
    {
        var services = new ServiceCollection();
        Should.Throw<InvalidOperationException>(() => services.AddCroniqJob<JobWithoutAttribute>());
    }

    [CroniqJob("core", "sample")]
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default) => Task.CompletedTask;
    }

    private sealed class JobWithoutAttribute : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default) => Task.CompletedTask;
    }

    private sealed class StubJobStore : IJobStore
    {
        public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken) =>
            Task.FromResult<IReadOnlyCollection<TriggerLease>>(Array.Empty<TriggerLease>());

        public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken) => Task.CompletedTask;

        public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken) => Task.CompletedTask;
    }
}
