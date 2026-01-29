using System.Diagnostics;
using Croniq.Api.Security;
using Croniq.Auth.Abstractions;
using Croniq.Core.Observability;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

internal sealed class RunnerPresenceGrpcService : RunnerPresence.RunnerPresenceBase
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Api.Grpc.RunnerPresence");
    private static readonly TimeSpan StreamPollInterval = TimeSpan.FromSeconds(10);
    private readonly IRunnerStore _runnerStore;
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly ILogger<RunnerPresenceGrpcService> _logger;

    public RunnerPresenceGrpcService(
        IRunnerStore runnerStore,
        ICallerContextAccessor callerAccessor,
        ILogger<RunnerPresenceGrpcService> logger)
    {
        _runnerStore = runnerStore ?? throw new ArgumentNullException(nameof(runnerStore));
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public override async Task Stream(
        RunnerPresenceStreamRequest request,
        IServerStreamWriter<RunnerPresenceStreamEvent> responseStream,
        ServerCallContext context)
    {
        using var activity = ActivitySource.StartActivity("Croniq.Grpc.RunnerPresence.Stream", ActivityKind.Server);

        try
        {
            var caller = _callerAccessor.Current;
            if (caller is null)
            {
                throw new RpcException(new Status(StatusCode.Unauthenticated, "caller context is not available."));
            }

            var tenantId = string.IsNullOrWhiteSpace(request.TenantId) ? caller.TenantId : request.TenantId.Trim();
            if (string.IsNullOrWhiteSpace(tenantId))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "tenant_id is required."));
            }

            var environmentTag = ResolveEnvironmentTag(request.EnvironmentTag, caller.EnvironmentTag);
            if (string.IsNullOrWhiteSpace(environmentTag))
            {
                throw new RpcException(new Status(StatusCode.InvalidArgument, "environment_tag is required."));
            }

            EnsureTenantOrThrow(TenantGuard.EnsureTenant(_callerAccessor, tenantId, environmentTag, CroniqScopes.RunnersRead));

            var scope = new PartitionScope(tenantId, environmentTag);
            var includeOffline = request.IncludeOffline;

            activity?.SetTag("croniq.tenant_id", IdentifierHashing.HashTenantId(tenantId));
            activity?.SetTag("croniq.environment", environmentTag);

            var previous = new Dictionary<string, RunnerPresenceSnapshot>(StringComparer.OrdinalIgnoreCase);
            var isFirst = true;

            while (!context.CancellationToken.IsCancellationRequested)
            {
                var pollStartedAt = DateTimeOffset.UtcNow;
                IReadOnlyCollection<RunnerStatus> runners;
                try
                {
                    runners = await _runnerStore
                        .ListAsync(new RunnerQuery(scope, pollStartedAt, includeOffline), context.CancellationToken)
                        .ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (context.CancellationToken.IsCancellationRequested)
                {
                    break;
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(
                        ex,
                        "Runner presence gRPC stream poll failed for {TenantId}/{EnvironmentTag}",
                        scope.TenantId,
                        scope.EnvironmentTag);
                    await Task.Delay(StreamPollInterval, context.CancellationToken).ConfigureAwait(false);
                    continue;
                }

                var current = runners
                    .Select(RunnerPresenceSnapshot.FromStatus)
                    .ToDictionary(snapshot => snapshot.RunnerId, StringComparer.OrdinalIgnoreCase);

                var totalCount = current.Count;
                var onlineCount = totalCount == 0 ? 0 : current.Values.Count(runner => runner.IsOnline);
                var latestSeenAtUtc = totalCount == 0
                    ? 0
                    : current.Values.Max(runner => runner.LastSeenAtUtc).ToUnixTimeMilliseconds();

                if (isFirst)
                {
                    var snapshot = current.Values.Select(ToRunnerPresenceRunner).ToArray();
                    var response = new RunnerPresenceStreamEvent
                    {
                        Type = "presence.snapshot",
                        EmittedAtUtc = pollStartedAt.ToUnixTimeMilliseconds(),
                        LatestSeenAtUtc = latestSeenAtUtc,
                        OnlineCount = onlineCount,
                        TotalCount = totalCount
                    };
                    response.Snapshot.AddRange(snapshot);
                    await responseStream.WriteAsync(response).ConfigureAwait(false);
                }
                else
                {
                    var updated = current.Values
                        .Where(entry => !previous.TryGetValue(entry.RunnerId, out var prior) || !entry.Equals(prior))
                        .Select(ToRunnerPresenceRunner)
                        .ToArray();
                    var removed = previous.Keys
                        .Where(key => !current.ContainsKey(key))
                        .ToArray();

                    var response = new RunnerPresenceStreamEvent
                    {
                        Type = "presence.delta",
                        EmittedAtUtc = pollStartedAt.ToUnixTimeMilliseconds(),
                        LatestSeenAtUtc = latestSeenAtUtc,
                        OnlineCount = onlineCount,
                        TotalCount = totalCount
                    };
                    if (updated.Length > 0)
                    {
                        response.Updated.AddRange(updated);
                    }
                    if (removed.Length > 0)
                    {
                        response.RemovedRunnerIds.AddRange(removed);
                    }

                    await responseStream.WriteAsync(response).ConfigureAwait(false);
                }

                previous = current;
                isFirst = false;

                await Task.Delay(StreamPollInterval, context.CancellationToken).ConfigureAwait(false);
            }

            activity?.SetStatus(ActivityStatusCode.Ok);
        }
        catch (Exception ex) when (GrpcDisconnects.IsExpected(ex, context.CancellationToken))
        {
            _logger.LogDebug("Runner presence gRPC stream closed.");
            activity?.SetStatus(ActivityStatusCode.Ok);
        }
        catch (RpcException)
        {
            activity?.SetStatus(ActivityStatusCode.Error);
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Runner presence gRPC stream failed.");
            activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
            throw new RpcException(new Status(StatusCode.Internal, ex.Message));
        }
    }

    private static string? ResolveEnvironmentTag(string? requestValue, string? callerValue)
    {
        if (!string.IsNullOrWhiteSpace(requestValue))
        {
            return requestValue.Trim();
        }

        return string.IsNullOrWhiteSpace(callerValue) ? null : callerValue.Trim();
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

    private sealed record RunnerPresenceSnapshot(
        string RunnerId,
        DateTimeOffset LastSeenAtUtc,
        DateTimeOffset ExpiresAtUtc,
        bool IsOnline,
        string? MetadataJson)
    {
        public static RunnerPresenceSnapshot FromStatus(RunnerStatus status) =>
            new(status.RunnerId, status.LastSeenAtUtc, status.ExpiresAtUtc, status.IsOnline, status.MetadataJson);
    }

    private static RunnerPresenceRunner ToRunnerPresenceRunner(RunnerPresenceSnapshot snapshot)
    {
        return new RunnerPresenceRunner
        {
            RunnerId = snapshot.RunnerId,
            LastSeenAtUtc = snapshot.LastSeenAtUtc.ToString("O"),
            ExpiresAtUtc = snapshot.ExpiresAtUtc.ToString("O"),
            IsOnline = snapshot.IsOnline,
            MetadataJson = snapshot.MetadataJson ?? string.Empty
        };
    }
}
