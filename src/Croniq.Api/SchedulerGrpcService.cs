using System.Globalization;
using Croniq.Auth.Abstractions;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

internal sealed class SchedulerGrpcService : Scheduler.SchedulerBase
{
    private readonly IJobRegistry _registry;
    private readonly IJobExecutionPipeline _pipeline;
    private readonly IPolicyResolver _policyResolver;
    private readonly IJobPersistenceProvider _store;
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly ILogger<SchedulerGrpcService> _logger;
    private readonly IPersistenceHealth? _health;

    public SchedulerGrpcService(
        IJobRegistry registry,
        IJobExecutionPipeline pipeline,
        IPolicyResolver policyResolver,
        IJobPersistenceProvider store,
        ICallerContextAccessor callerAccessor,
        ILogger<SchedulerGrpcService> logger,
        IPersistenceHealth? health = null)
    {
        _registry = registry ?? throw new ArgumentNullException(nameof(registry));
        _pipeline = pipeline ?? throw new ArgumentNullException(nameof(pipeline));
        _policyResolver = policyResolver ?? throw new ArgumentNullException(nameof(policyResolver));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _health = health;
    }

    public override async Task<HealthResponse> Health(HealthRequest request, ServerCallContext context)
    {
        var providerName = _store.GetType().FullName ?? "unknown";
        if (_health is null)
        {
            return new HealthResponse { Status = "ok", Provider = providerName, Note = "no-db-provider-configured" };
        }

        try
        {
            var result = await _health.CheckAsync(context.CancellationToken).ConfigureAwait(false);
            return result.IsHealthy
                ? new HealthResponse { Status = "ok", Provider = providerName, Db = "reachable" }
                : new HealthResponse { Status = "unhealthy", Provider = providerName, Db = "unreachable", Detail = result.Detail };
        }
        catch (Exception ex)
        {
            return new HealthResponse { Status = "unhealthy", Provider = providerName, Db = "unreachable", Detail = ex.Message };
        }
    }

    public override async Task<TriggerJobResponse> TriggerJob(TriggerJobRequest request, ServerCallContext context)
    {
        try
        {
            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "job_key must follow the Croniq format."));
            }

            EnsureTenantOrThrow(TenantGuard.EnsureJobScope(_callerAccessor, jobKey, CroniqScopes.JobsTrigger));

            if (!_registry.TryGet(jobKey, out var descriptor))
            {
                throw new RpcException(new Status(StatusCode.NotFound, "job_not_registered"));
            }

            var metadata = request.Metadata.Count == 0
                ? new Dictionary<string, string>()
                : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);

            var executionOptions = _policyResolver.ResolveExecution(jobKey);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, descriptor, executionOptions, metadata, activitySource: null);

            await _pipeline.ExecuteAsync(execRequest, context.CancellationToken).ConfigureAwait(false);
            return new TriggerJobResponse { Status = "triggered" };
        }
        catch (RpcException)
        {
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "trigger-job failed for {JobKey}", request.JobKey);
            throw new RpcException(new Status(StatusCode.Internal, ex.Message));
        }
    }

    public override async Task<UpsertScheduleResponse> UpsertSchedule(UpsertScheduleRequest request, ServerCallContext context)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(request.JobKey) || string.IsNullOrWhiteSpace(request.CronExpression))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "job_key and cron_expression are required."));
            }

            if (!JobKey.TryParse(request.JobKey, out var jobKey))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "job_key must follow the Croniq format."));
            }

            EnsureTenantOrThrow(TenantGuard.EnsureJobScope(_callerAccessor, jobKey, CroniqScopes.SchedulesWrite));

            var triggerId = string.IsNullOrWhiteSpace(request.TriggerId)
                ? $"{request.JobKey}:{request.CronExpression}"
                : request.TriggerId;

            var scope = new PartitionScope(jobKey.TenantId, jobKey.EnvironmentTag);
            var metadata = request.Metadata.Count == 0
                ? null
                : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);

            var (startAt, endAt) = ParseScheduleWindow(request.StartAtUtc, request.EndAtUtc);
            var enabled = request.HasEnabled ? request.Enabled : true;

            var job = new JobDefinition(
                jobKey.Value,
                jobKey.NamespaceSegment,
                jobKey.JobName,
                jobKey.Variant,
                request.Description,
                metadata);

            var trigger = new TriggerDefinition(
                triggerId,
                jobKey.Value,
                request.CronExpression,
                scope,
                startAt,
                endAt,
                enabled,
                metadata);

            await _store.UpsertJobAsync(job, context.CancellationToken).ConfigureAwait(false);
            await _store.UpsertTriggerAsync(trigger, context.CancellationToken).ConfigureAwait(false);

            return new UpsertScheduleResponse
            {
                TriggerId = trigger.TriggerId,
                JobKey = trigger.JobKey,
                ScheduleExpression = trigger.ScheduleExpression
            };
        }
        catch (RpcException)
        {
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "upsert-schedule failed for {JobKey}", request.JobKey);
            throw new RpcException(new Status(StatusCode.Internal, ex.Message));
        }
    }

    public override async Task<DeleteScheduleResponse> DeleteSchedule(DeleteScheduleRequest request, ServerCallContext context)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(request.TriggerId))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "trigger_id is required."));
            }

            if (string.IsNullOrWhiteSpace(request.TenantId))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "tenant_id is required."));
            }

            if (string.IsNullOrWhiteSpace(request.EnvironmentTag))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "environment_tag is required."));
            }

            EnsureTenantOrThrow(TenantGuard.EnsureTenant(_callerAccessor, request.TenantId, request.EnvironmentTag, CroniqScopes.SchedulesWrite));

            var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
            await _store.DeleteTriggerAsync(request.TriggerId, scope, context.CancellationToken).ConfigureAwait(false);
            return new DeleteScheduleResponse { Status = "deleted" };
        }
        catch (RpcException)
        {
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "delete-schedule failed for {TriggerId}", request.TriggerId);
            throw new RpcException(new Status(StatusCode.Internal, ex.Message));
        }
    }

    private static (DateTimeOffset?, DateTimeOffset?) ParseScheduleWindow(string startAt, string endAt)
    {
        DateTimeOffset? start = null;
        DateTimeOffset? end = null;

        if (!string.IsNullOrWhiteSpace(startAt) && DateTimeOffset.TryParse(startAt, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var parsedStart))
        {
            start = parsedStart;
        }

        if (!string.IsNullOrWhiteSpace(endAt) && DateTimeOffset.TryParse(endAt, CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var parsedEnd))
        {
            end = parsedEnd;
        }

        return (start, end);
    }

    private static void EnsureTenantOrThrow(IResult? failure)
    {
        if (failure is null)
        {
            return;
        }

        var status = StatusCode.PermissionDenied;
        var detail = "tenant_mismatch";

        if (failure is IStatusCodeHttpResult statusResult)
        {
            if (statusResult.StatusCode == StatusCodes.Status401Unauthorized)
            {
                status = StatusCode.Unauthenticated;
                detail = "unauthenticated";
            }
        }

        throw new RpcException(new Status(status, detail));
    }
}
