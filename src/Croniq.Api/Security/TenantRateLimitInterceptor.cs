using System.Collections.Concurrent;
using System.Globalization;
using System.Linq;
using System.Threading.RateLimiting;
using Croniq.Auth.Abstractions;
using Grpc.Core;
using Grpc.Core.Interceptors;
using Microsoft.Extensions.Logging;

namespace Croniq.Api.Security;

internal sealed class TenantRateLimitInterceptor : Interceptor
{
    private readonly TenantRateLimitDecider _decider;
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly ILogger<TenantRateLimitInterceptor> _logger;
    private readonly ConcurrentDictionary<string, LimiterEntry> _limiters = new(StringComparer.Ordinal);

    public TenantRateLimitInterceptor(
        TenantRateLimitDecider decider,
        ICallerContextAccessor callerAccessor,
        ILogger<TenantRateLimitInterceptor> logger)
    {
        _decider = decider ?? throw new ArgumentNullException(nameof(decider));
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public override async Task<TResponse> UnaryServerHandler<TRequest, TResponse>(
        TRequest request,
        ServerCallContext context,
        UnaryServerMethod<TRequest, TResponse> continuation)
    {
        using var lease = await AcquireLeaseAsync(context).ConfigureAwait(false);
        return await continuation(request, context).ConfigureAwait(false);
    }

    public override async Task<TResponse> ClientStreamingServerHandler<TRequest, TResponse>(
        IAsyncStreamReader<TRequest> requestStream,
        ServerCallContext context,
        ClientStreamingServerMethod<TRequest, TResponse> continuation)
    {
        using var lease = await AcquireLeaseAsync(context).ConfigureAwait(false);
        return await continuation(requestStream, context).ConfigureAwait(false);
    }

    public override async Task ServerStreamingServerHandler<TRequest, TResponse>(
        TRequest request,
        IServerStreamWriter<TResponse> responseStream,
        ServerCallContext context,
        ServerStreamingServerMethod<TRequest, TResponse> continuation)
    {
        using var lease = await AcquireLeaseAsync(context).ConfigureAwait(false);
        await continuation(request, responseStream, context).ConfigureAwait(false);
    }

    public override async Task DuplexStreamingServerHandler<TRequest, TResponse>(
        IAsyncStreamReader<TRequest> requestStream,
        IServerStreamWriter<TResponse> responseStream,
        ServerCallContext context,
        DuplexStreamingServerMethod<TRequest, TResponse> continuation)
    {
        using var lease = await AcquireLeaseAsync(context).ConfigureAwait(false);
        await continuation(requestStream, responseStream, context).ConfigureAwait(false);
    }

    private async Task<RateLimitLease> AcquireLeaseAsync(ServerCallContext context)
    {
        var caller = _callerAccessor.Current;
        var fallback = ResolveFallback(context);
        var partitionId = _decider.GetPartitionId(caller, fallback);
        var permits = _decider.GetPermitLimit(caller);
        var limiter = GetOrCreateLimiter(partitionId, permits);

        var lease = await limiter.AcquireAsync(permitCount: 1, context.CancellationToken).ConfigureAwait(false);
        if (lease.IsAcquired)
        {
            return lease;
        }

        _logger.LogWarning("gRPC rate limit exceeded for {Partition}", partitionId);
        var status = new Status(StatusCode.ResourceExhausted, "Rate limit exceeded");

        if (lease.TryGetMetadata(MetadataName.RetryAfter, out var retryAfter) && retryAfter is TimeSpan retry)
        {
            context.ResponseTrailers.Add("retry-after", retry.TotalSeconds.ToString(CultureInfo.InvariantCulture));
            throw new RpcException(status, $"retry-after={retry.TotalSeconds:F0}s");
        }

        throw new RpcException(status);
    }

    private static string ResolveFallback(ServerCallContext context)
    {
        var metadata = context.RequestHeaders;
        if (metadata is not null)
        {
            var apiKeyHeader = metadata.FirstOrDefault(entry => string.Equals(entry.Key, "x-croniq-key", StringComparison.OrdinalIgnoreCase));
            if (apiKeyHeader is not null && !string.IsNullOrWhiteSpace(apiKeyHeader.Value))
            {
                return apiKeyHeader.Value;
            }
        }

        return context.Peer ?? "grpc";
    }

    private FixedWindowRateLimiter GetOrCreateLimiter(string partitionId, int permits)
    {
        if (permits <= 0)
        {
            permits = 1;
        }

        while (true)
        {
            if (_limiters.TryGetValue(partitionId, out var existing))
            {
                if (existing.PermitLimit == permits)
                {
                    return existing.Limiter;
                }

                var replacement = CreateLimiterEntry(permits);
                if (_limiters.TryUpdate(partitionId, replacement, existing))
                {
                    existing.Dispose();
                    return replacement.Limiter;
                }

                replacement.Dispose();
                continue;
            }

            var created = CreateLimiterEntry(permits);
            if (_limiters.TryAdd(partitionId, created))
            {
                return created.Limiter;
            }

            created.Dispose();
        }
    }

    private static LimiterEntry CreateLimiterEntry(int permits)
    {
        var limiter = new FixedWindowRateLimiter(new FixedWindowRateLimiterOptions
        {
            PermitLimit = permits,
            Window = TimeSpan.FromMinutes(1),
            QueueLimit = permits,
            QueueProcessingOrder = QueueProcessingOrder.OldestFirst
        });

        return new LimiterEntry(limiter, permits);
    }

    private sealed record LimiterEntry(FixedWindowRateLimiter Limiter, int PermitLimit)
    {
        public void Dispose() => Limiter.Dispose();
    }
}
