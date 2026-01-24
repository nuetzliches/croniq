using System.Diagnostics;
using Croniq.Auth.Abstractions;
using Croniq.Core.Observability;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

internal sealed class WebhookActivityGrpcService : WebhookActivity.WebhookActivityBase
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Api.Grpc.WebhookActivity");
    private static readonly TimeSpan StreamPollInterval = TimeSpan.FromSeconds(5);
    private readonly IWebhookActivityStore? _activityStore;
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly ILogger<WebhookActivityGrpcService> _logger;

    public WebhookActivityGrpcService(
        IWebhookActivityStore? activityStore,
        ICallerContextAccessor callerAccessor,
        ILogger<WebhookActivityGrpcService> logger)
    {
        _activityStore = activityStore;
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public override async Task Stream(
        WebhookActivityStreamRequest request,
        IServerStreamWriter<WebhookActivityStreamEvent> responseStream,
        ServerCallContext context)
    {
        using var activity = ActivitySource.StartActivity("Croniq.Grpc.WebhookActivity.Stream", ActivityKind.Server);

        try
        {
            if (_activityStore is null)
            {
                throw new RpcException(new Status(StatusCode.Unavailable, "webhook-activity-unavailable"));
            }

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

            EnsureTenantOrThrow(TenantGuard.EnsureTenant(_callerAccessor, tenantId, environmentTag, CroniqScopes.WebhooksRead));

            var query = BuildQuery(request);
            var scope = new PartitionScope(tenantId, environmentTag);

            activity?.SetTag("croniq.tenant_id", IdentifierHashing.HashTenantId(tenantId));
            activity?.SetTag("croniq.environment", environmentTag);

            var lastSeenUtc = query.UpdatedSinceUtc ?? query.FromUtc ?? DateTimeOffset.UtcNow;
            if (query.ToUtc.HasValue && query.ToUtc.Value < lastSeenUtc)
            {
                lastSeenUtc = query.ToUtc.Value;
            }

            while (!context.CancellationToken.IsCancellationRequested)
            {
                var pollStartedAt = DateTimeOffset.UtcNow;
                var probeQuery = new WebhookActivityQuery
                {
                    FromUtc = query.FromUtc,
                    ToUtc = query.ToUtc,
                    UpdatedSinceUtc = lastSeenUtc,
                    HookKeys = query.HookKeys,
                    JobKeys = query.JobKeys,
                    Limit = query.Limit
                }.Normalize();

                IReadOnlyCollection<WebhookActivityEntry> entries;
                try
                {
                    entries = await _activityStore.ListAsync(scope, probeQuery, context.CancellationToken).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (context.CancellationToken.IsCancellationRequested)
                {
                    break;
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(
                        ex,
                        "webhook activity gRPC stream poll failed for {TenantId}/{EnvironmentTag}",
                        scope.TenantId,
                        scope.EnvironmentTag);
                    await Task.Delay(StreamPollInterval, context.CancellationToken).ConfigureAwait(false);
                    continue;
                }

                if (entries.Count > 0)
                {
                    var latestOccurredAtUtc = entries.Max(entry => entry.OccurredAtUtc);
                    await responseStream.WriteAsync(new WebhookActivityStreamEvent
                    {
                        Type = "activity.updated",
                        EmittedAtUtc = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
                        LatestOccurredAtUtc = latestOccurredAtUtc.ToUnixTimeMilliseconds()
                    }).ConfigureAwait(false);
                }

                lastSeenUtc = pollStartedAt;
                await Task.Delay(StreamPollInterval, context.CancellationToken).ConfigureAwait(false);
            }

            activity?.SetStatus(ActivityStatusCode.Ok);
        }
        catch (Exception ex) when (GrpcDisconnects.IsExpected(ex, context.CancellationToken))
        {
            _logger.LogDebug("Webhook activity gRPC stream closed.");
            activity?.SetStatus(ActivityStatusCode.Ok);
        }
        catch (RpcException)
        {
            activity?.SetStatus(ActivityStatusCode.Error);
            throw;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "Webhook activity gRPC stream failed.");
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

    private static WebhookActivityQuery BuildQuery(WebhookActivityStreamRequest request)
    {
        var fromUtc = ResolveTimestamp(request.FromUtc, "from_utc");
        var toUtc = ResolveTimestamp(request.ToUtc, "to_utc");
        var updatedSinceUtc = ResolveTimestamp(request.UpdatedSinceUtc, "updated_since_utc");

        if (fromUtc.HasValue && toUtc.HasValue && fromUtc > toUtc)
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, "from_utc must be earlier than to_utc."));
        }

        var hookKeys = NormalizeKeys(request.HookKeys);
        var jobKeys = NormalizeKeys(request.JobKeys);
        var limit = request.Limit > 0 ? request.Limit : 1;

        return new WebhookActivityQuery
        {
            FromUtc = fromUtc,
            ToUtc = toUtc,
            UpdatedSinceUtc = updatedSinceUtc,
            HookKeys = hookKeys,
            JobKeys = jobKeys,
            Limit = limit
        }.Normalize();
    }

    private static DateTimeOffset? ResolveTimestamp(long value, string field)
    {
        if (value <= 0)
        {
            return null;
        }

        try
        {
            return DateTimeOffset.FromUnixTimeMilliseconds(value);
        }
        catch (ArgumentOutOfRangeException)
        {
            throw new RpcException(new Status(StatusCode.InvalidArgument, $"{field} must be a valid unix timestamp (ms)."));
        }
    }

    private static IReadOnlyCollection<string>? NormalizeKeys(IEnumerable<string> values)
    {
        if (values is null)
        {
            return null;
        }

        var normalized = values
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Select(value => value.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return normalized.Length == 0 ? null : normalized;
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
