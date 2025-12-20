using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Sdk;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Jobs;

public class JobRegistrationTests
{
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
        {
            return Task.CompletedTask;
        }
    }

    [Fact]
    public void JobRegistration_Throws_WhenJobTypeIsNull()
    {
        Should.Throw<ArgumentNullException>(() => new JobRegistration(null!));
    }

    [Fact]
    public void JobRegistration_StoresJobType()
    {
        var registration = new JobRegistration(typeof(SampleJob));

        registration.JobType.ShouldBe(typeof(SampleJob));
    }

    [Fact]
    public void JobRegistration_Generic_UsesGenericType()
    {
        var registration = new JobRegistration<SampleJob>();

        registration.JobType.ShouldBe(typeof(SampleJob));
    }

    [Fact]
    public void FluentJobRegistration_RequiresAttribute()
    {
        Should.Throw<ArgumentNullException>(() => new FluentJobRegistration(typeof(SampleJob), null!));
    }

    [Fact]
    public void FluentJobRegistration_ExposesAttribute()
    {
        var attribute = new CroniqJobAttribute("samples", "demo");
        var registration = new FluentJobRegistration(typeof(SampleJob), attribute);

        registration.Attribute.ShouldBe(attribute);
    }
}
