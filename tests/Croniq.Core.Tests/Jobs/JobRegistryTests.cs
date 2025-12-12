using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Sdk;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Jobs;

public class JobRegistryTests
{
    [CroniqJob("samples", "demo")]
    private sealed class SampleJob : IJob
    {
        [CroniqJob("samples", "demo")]
        public class Handler : IJob
        {
            public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
            {
                return Task.CompletedTask;
            }
        }

        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
        {
            return Task.CompletedTask;
        }
    }

    private sealed class JobWithoutAttribute : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
        {
            return Task.CompletedTask;
        }
    }

    [Fact]
    public void Registers_jobs_with_composed_job_key()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t1", EnvironmentTag = "dev" });
        var registrations = new List<JobRegistration>
        {
            new JobRegistration(typeof(SampleJob.Handler))
        };

        var registry = new JobRegistry(options, registrations);

        registry.Descriptors.ShouldHaveSingleItem();
        registry.TryGet(JobKey.Create("t1", "dev", "samples", "demo"), out var descriptor).ShouldBeTrue();
        descriptor!.JobType.ShouldBe(typeof(SampleJob.Handler));
        descriptor.JobKey.Value.ShouldBe("t1:dev:samples:demo");
    }

    [Fact]
    public void Throws_when_job_is_missing_attribute()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions());
        var registrations = new List<JobRegistration>
        {
            new JobRegistration(typeof(JobWithoutAttribute))
        };

        Should.Throw<InvalidOperationException>(() => new JobRegistry(options, registrations));
    }

    [Fact]
    public void Throws_on_duplicate_job_keys()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "t1", EnvironmentTag = "dev" });
        var registrations = new List<JobRegistration>
        {
            new JobRegistration(typeof(SampleJob.Handler)),
            new JobRegistration(typeof(SampleJob.Handler))
        };

        Should.Throw<InvalidOperationException>(() => new JobRegistry(options, registrations));
    }
}
