using System;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class SchedulerGrpcTests
{
    private readonly RecordingJobExecutionPipeline _pipeline = new();
    private readonly FakeJobRegistry _registry = new();
    private readonly FakePolicyResolver _policies = new();
    private readonly NoopJobPersistenceProvider _store = new();
    private readonly CallerContextAccessor _callerAccessor = new();
    private readonly SchedulerGrpcService _service;

    public SchedulerGrpcTests()
    {
        _callerAccessor.Current = new CallerContext(
            TestCallerContextFactory.DefaultTenantId,
            TestCallerContextFactory.DefaultEnvironment,
            CallerType.ApiKey,
            CallerId: "itest-client",
            Scopes: new[]
            {
                CroniqScopes.JobsTrigger,
                CroniqScopes.SchedulesWrite
            });

        _service = new SchedulerGrpcService(
            _registry,
            _pipeline,
            _policies,
            _store,
            _callerAccessor,
            NullLogger<SchedulerGrpcService>.Instance,
            _store);
    }

    [Fact]
    public async Task TriggerJob_Executes_WhenTenantMatches()
    {
        var jobKey = $"{TestCallerContextFactory.DefaultTenantId}:{TestCallerContextFactory.DefaultEnvironment}:ops:smoke";
        _registry.EnsureJob(jobKey);

        var response = await _service.TriggerJob(new TriggerJobRequest { JobKey = jobKey }, CreateContext());

        response.Status.ShouldBe("triggered");
        _pipeline.Executions.Count.ShouldBe(1);
        _pipeline.Executions[0].JobKey.ShouldBe(JobKey.Create("tenant-itest", "dev", "ops", "smoke"));
    }

    [Fact]
    public async Task TriggerJob_DeniesCrossTenant()
    {
        var jobKey = $"{TestCallerContextFactory.DefaultTenantId}:{TestCallerContextFactory.DefaultEnvironment}:ops:smoke";
        _registry.EnsureJob(jobKey);
        _callerAccessor.Current = new CallerContext("other", TestCallerContextFactory.DefaultEnvironment, CallerType.ApiKey, "client", new[] { CroniqScopes.JobsTrigger });

        var ex = await Should.ThrowAsync<RpcException>(async () =>
            await _service.TriggerJob(new TriggerJobRequest { JobKey = jobKey }, CreateContext()));
        ex.StatusCode.ShouldBe(StatusCode.PermissionDenied);
    }

    [Fact]
    public async Task UpsertSchedule_Succeeds_WithTenantMatch()
    {
        var jobKey = $"{TestCallerContextFactory.DefaultTenantId}:{TestCallerContextFactory.DefaultEnvironment}:ops:plan";
        _registry.EnsureJob(jobKey);

        var response = await _service.UpsertSchedule(new UpsertScheduleRequest
        {
            JobKey = jobKey,
            CronExpression = "0/5 * * * * ?",
            Description = "grpc-schedule"
        }, CreateContext());

        response.TriggerId.ShouldNotBeNullOrWhiteSpace();
        response.JobKey.ShouldBe(jobKey);
        response.ScheduleExpression.ShouldBe("0/5 * * * * ?");
    }

    [Fact]
    public async Task DeleteSchedule_DeniesCrossTenant()
    {
        _callerAccessor.Current = new CallerContext("other", TestCallerContextFactory.DefaultEnvironment, CallerType.ApiKey, "client", new[] { CroniqScopes.SchedulesWrite });

        var ex = await Should.ThrowAsync<RpcException>(async () =>
            await _service.DeleteSchedule(new DeleteScheduleRequest
            {
                TriggerId = "t1",
                TenantId = TestCallerContextFactory.DefaultTenantId,
                EnvironmentTag = TestCallerContextFactory.DefaultEnvironment
            }, CreateContext()));
        ex.StatusCode.ShouldBe(StatusCode.PermissionDenied);
    }

    [Fact]
    public async Task DeleteSchedule_Succeeds_WhenTenantMatches()
    {
        var response = await _service.DeleteSchedule(new DeleteScheduleRequest
        {
            TriggerId = "t1",
            TenantId = TestCallerContextFactory.DefaultTenantId,
            EnvironmentTag = TestCallerContextFactory.DefaultEnvironment
        }, CreateContext());

        response.Status.ShouldBe("deleted");
    }

    private static ServerCallContext CreateContext()
    {
        return new FakeServerCallContext();
    }
}

internal sealed class FakeServerCallContext : ServerCallContext
{
    private readonly Metadata _requestHeaders = new();
    private readonly Metadata _responseTrailers = new();
    private Status _status;

    protected override string MethodCore => "test";
    protected override string HostCore => "localhost";
    protected override string PeerCore => "127.0.0.1";
    protected override DateTime DeadlineCore => DateTime.UtcNow.AddMinutes(1);
    protected override Metadata RequestHeadersCore => _requestHeaders;
    protected override CancellationToken CancellationTokenCore => CancellationToken.None;
    protected override Metadata ResponseTrailersCore => _responseTrailers;
    protected override Status StatusCore { get => _status; set => _status = value; }
    protected override WriteOptions? WriteOptionsCore { get; set; }
    protected override AuthContext AuthContextCore => new(string.Empty, new Dictionary<string, List<AuthProperty>>());

    protected override ContextPropagationToken CreatePropagationTokenCore(ContextPropagationOptions? options) => throw new NotSupportedException();
    protected override Task WriteResponseHeadersAsyncCore(Metadata responseHeaders) => Task.CompletedTask;
}
