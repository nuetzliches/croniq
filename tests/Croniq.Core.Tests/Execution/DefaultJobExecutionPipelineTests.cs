using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using NSubstitute;
using Polly;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class DefaultJobExecutionPipelineTests
{
    [Fact]
    public async Task Executes_job_and_forwards_metadata()
    {
        var services = new ServiceCollection();
        services.AddLogging();
        services.AddSingleton<TestJob>();
        var provider = services.BuildServiceProvider();

        var scopeFactory = provider.GetRequiredService<IServiceScopeFactory>();
        var policyResolver = Substitute.For<IPolicyResolver>();
        policyResolver.ResolveExecution(Arg.Any<JobKey>(), Arg.Any<PartitionScope?>()).Returns(new ExecutionPolicyOptions());

        var pipelineProvider = Substitute.For<IExecutionPolicyPipelineProvider>();
        pipelineProvider.Get(Arg.Any<JobKey>(), Arg.Any<ExecutionPolicyOptions>())
            .Returns(new ResiliencePipelineBuilder().Build());

        var pipeline = new DefaultJobExecutionPipeline(
            scopeFactory,
            new ActivitySource("test"),
            policyResolver,
            pipelineProvider,
            NullLogger<DefaultJobExecutionPipeline>.Instance);

        var jobKey = JobKey.Create("ns", "job");
        var scope = new PartitionScope("tenant", "env");
        var descriptor = new JobDescriptor(typeof(TestJob), new CroniqJobAttribute("ns", "job"), jobKey);
        var metadata = new Dictionary<string, string>
        {
            { "trigger_id", "t-1" },
            { "initiator", "user" }
        };
        var request = new JobExecutionRequest("exec-123", jobKey, scope, descriptor, null, metadata, new ActivitySource("job"));

        await pipeline.ExecuteAsync(request, CancellationToken.None);

        var job = provider.GetRequiredService<TestJob>();
        job.Executions.ShouldBe(1);
        job.LastContext.ShouldNotBeNull();
        job.LastContext.ExecutionId.ShouldBe("exec-123");
        job.LastContext.JobKey.ShouldBe(jobKey.ToString());
        job.LastContext.Metadata["trigger_id"].ShouldBe("t-1");
        job.LastContext.Metadata["initiator"].ShouldBe("user");
    }

    [Fact]
    public async Task Throws_when_job_fails()
    {
        var services = new ServiceCollection();
        services.AddLogging();
        services.AddSingleton<FaultyJob>();
        var provider = services.BuildServiceProvider();

        var scopeFactory = provider.GetRequiredService<IServiceScopeFactory>();
        var policyResolver = Substitute.For<IPolicyResolver>();
        policyResolver.ResolveExecution(Arg.Any<JobKey>(), Arg.Any<PartitionScope?>()).Returns(new ExecutionPolicyOptions());

        var pipelineProvider = Substitute.For<IExecutionPolicyPipelineProvider>();
        pipelineProvider.Get(Arg.Any<JobKey>(), Arg.Any<ExecutionPolicyOptions>())
            .Returns(new ResiliencePipelineBuilder().Build());

        var pipeline = new DefaultJobExecutionPipeline(
            scopeFactory,
            new ActivitySource("test"),
            policyResolver,
            pipelineProvider,
            NullLogger<DefaultJobExecutionPipeline>.Instance);

        var jobKey = JobKey.Create("ns", "faulty");
        var scope = new PartitionScope("tenant", "env");
        var descriptor = new JobDescriptor(typeof(FaultyJob), new CroniqJobAttribute("ns", "faulty"), jobKey);
        var request = new JobExecutionRequest("exec-faulty", jobKey, scope, descriptor, null, new Dictionary<string, string>(), new ActivitySource("job"));

        await Should.ThrowAsync<InvalidOperationException>(() => pipeline.ExecuteAsync(request, CancellationToken.None));
    }

    [CroniqJob("ns", "job")]
    private sealed class TestJob : IJob
    {
        public int Executions { get; private set; }
        public IJobExecutionContext? LastContext { get; private set; }

        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
        {
            Executions++;
            LastContext = context;
            return Task.CompletedTask;
        }
    }

    [CroniqJob("ns", "faulty")]
    private sealed class FaultyJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default)
            => throw new InvalidOperationException("boom");
    }
}
