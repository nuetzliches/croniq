using System.Collections.Concurrent;
using System.Globalization;
using System.Linq;
using System.Threading.RateLimiting;
using Croniq.Auth.Abstractions;
using Grpc.Core;
using Grpc.Core.Interceptors;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Api.Security;

internal sealed class TenantRateLimitInterceptor : Interceptor
{
    private readonly TenantRateLimitDecider _decider;
    private readonly ICallerContextAccessor _callerAccessor;
    private readonly ILogger<TenantRateLimitInterceptor> _logger;
    private readonly IOptionsMonitor<CroniqApiOptions> _apiOptions;
    private readonly ConcurrentDictionary<string, LimiterEntry> _limiters = new(StringComparer.Ordinal);
    private long _lastCleanupTicks;

    public TenantRateLimitInterceptor(
        TenantRateLimitDecider decider,
        ICallerContextAccessor callerAccessor,
        IOptionsMonitor<CroniqApiOptions> apiOptions,
        ILogger<TenantRateLimitInterceptor> logger)
    {
        _decider = decider ?? throw new ArgumentNullException(nameof(decider));
        _callerAccessor = callerAccessor ?? throw new ArgumentNullException(nameof(callerAccessor));
        _apiOptions = apiOptions ?? throw new ArgumentNullException(nameof(apiOptions));
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
        var nowUtc = DateTimeOffset.UtcNow;
        var limiter = GetOrCreateLimiter(partitionId, permits, nowUtc);
        TryCleanupLimiters(nowUtc);

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

    private FixedWindowRateLimiter GetOrCreateLimiter(string partitionId, int permits, DateTimeOffset nowUtc)
    {
        if (permits <= 0)
        {
            permits = 1;
        }

        while (true)
        {
            if (_limiters.TryGetValue(partitionId, out var existing))
            {
                existing.Touch(nowUtc);
                if (existing.PermitLimit == permits)
                {
                    return existing.Limiter;
                }

                var replacement = CreateLimiterEntry(permits, nowUtc);
                if (_limiters.TryUpdate(partitionId, replacement, existing))
                {
                    existing.Dispose();
                    return replacement.Limiter;
                }

                replacement.Dispose();
                continue;
            }

            var created = CreateLimiterEntry(permits, nowUtc);
            if (_limiters.TryAdd(partitionId, created))
            {
                return created.Limiter;
            }

            created.Dispose();
        }
    }

    private static LimiterEntry CreateLimiterEntry(int permits, DateTimeOffset nowUtc)
    {
        var limiter = new FixedWindowRateLimiter(new FixedWindowRateLimiterOptions
        {
            PermitLimit = permits,
            Window = TimeSpan.FromMinutes(1),
            QueueLimit = permits,
            QueueProcessingOrder = QueueProcessingOrder.OldestFirst
        });

        return new LimiterEntry(limiter, permits, nowUtc);
    }

    private void TryCleanupLimiters(DateTimeOffset nowUtc)
    {
        var options = _apiOptions.CurrentValue ?? new CroniqApiOptions();
        var interval = options.RateLimiterCacheCleanupInterval;
        var retention = options.RateLimiterCacheRetention;
        if (interval <= TimeSpan.Zero || retention <= TimeSpan.Zero)
        {
            return;
        }

        var nowTicks = nowUtc.UtcTicks;
        var lastTicks = Interlocked.Read(ref _lastCleanupTicks);
        if (nowTicks - lastTicks < interval.Ticks)
        {
            return;
        }

        if (Interlocked.CompareExchange(ref _lastCleanupTicks, nowTicks, lastTicks) != lastTicks)
        {
            return;
        }

        foreach (var entry in _limiters)
        {
            if (nowUtc - entry.Value.LastUsedUtc < retention)
            {
                continue;
            }

            if (_limiters.TryRemove(entry.Key, out var removed))
            {
                removed.Dispose();
            }
        }
    }

    private sealed class LimiterEntry
    {
        private long _lastUsedTicks;

        public LimiterEntry(FixedWindowRateLimiter limiter, int permitLimit, DateTimeOffset createdAtUtc)
        {
            Limiter = limiter;
            PermitLimit = permitLimit;
            _lastUsedTicks = createdAtUtc.UtcTicks;
        }

        public FixedWindowRateLimiter Limiter { get; }
        public int PermitLimit { get; }
        public DateTimeOffset LastUsedUtc => new DateTimeOffset(Interlocked.Read(ref _lastUsedTicks), TimeSpan.Zero);

        public void Touch(DateTimeOffset nowUtc)
        {
            Interlocked.Exchange(ref _lastUsedTicks, nowUtc.UtcTicks);
        }

        public void Dispose() => Limiter.Dispose();
    }
}
