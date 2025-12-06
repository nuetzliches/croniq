using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Microsoft.Extensions.Logging;
using Polly;
using Polly.CircuitBreaker;
using Polly.Retry;
using Polly.Timeout;

namespace Croniq.Core.Execution;

public sealed class ExecutionPolicyPipelineProvider : IExecutionPolicyPipelineProvider
{
    private readonly ConcurrentDictionary<string, CachedPipeline> _pipelines = new(StringComparer.OrdinalIgnoreCase);
    private readonly ILogger<ExecutionPolicyPipelineProvider> _logger;

    public ExecutionPolicyPipelineProvider(ILogger<ExecutionPolicyPipelineProvider> logger)
    {
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public ResiliencePipeline Get(JobKey jobKey, ExecutionPolicyOptions options)
    {
        if (options is null)
        {
            throw new ArgumentNullException(nameof(options));
        }

        var fingerprint = CreateFingerprint(options);
        var cacheKey = jobKey.Value;

        var cached = _pipelines.AddOrUpdate(
            cacheKey,
            _ => new CachedPipeline(BuildPipeline(cacheKey, options), fingerprint),
            (_, existing) => existing.Fingerprint == fingerprint
                ? existing
                : new CachedPipeline(BuildPipeline(cacheKey, options), fingerprint));

        return cached.Pipeline;
    }

    private ResiliencePipeline BuildPipeline(string cacheKey, ExecutionPolicyOptions options)
    {
        var builder = new ResiliencePipelineBuilder();

        if (options.Timeout.Enabled && options.Timeout.Timeout > TimeSpan.Zero)
        {
            builder.AddTimeout(new TimeoutStrategyOptions
            {
                Timeout = options.Timeout.Timeout,
                OnTimeout = args =>
                {
                    _logger.LogWarning("Job {JobKey} timed out after {Timeout}", cacheKey, args.Timeout);
                    return default;
                }
            });
        }

        if (options.CircuitBreaker.Enabled)
        {
            builder.AddCircuitBreaker(new CircuitBreakerStrategyOptions
            {
                FailureRatio = Math.Clamp(options.CircuitBreaker.FailureThreshold / 100d, 0.01d, 1d),
                SamplingDuration = options.CircuitBreaker.SamplingWindow,
                BreakDuration = options.CircuitBreaker.BreakDuration,
                MinimumThroughput = Math.Max(2, options.CircuitBreaker.MinimumThroughput),
                ShouldHandle = BuildCircuitBreakerPredicate(options.Retry.RetryableExceptions)
            });
        }

        if (options.Retry.Enabled && options.Retry.MaxAttempts > 1)
        {
            builder.AddRetry(new RetryStrategyOptions
            {
                MaxRetryAttempts = Math.Max(1, options.Retry.MaxAttempts - 1),
                Delay = options.Retry.InitialDelay,
                MaxDelay = options.Retry.MaxDelay,
                BackoffType = MapBackoff(options.Retry.BackoffStrategy),
                UseJitter = options.Retry.JitterFactor > 0,
                ShouldHandle = BuildRetryPredicate(options.Retry.RetryableExceptions)
            });
        }

        return builder.Build();
    }

    private static DelayBackoffType MapBackoff(RetryBackoffStrategy strategy) => strategy switch
    {
        RetryBackoffStrategy.Linear => DelayBackoffType.Linear,
        RetryBackoffStrategy.Exponential => DelayBackoffType.Exponential,
        _ => DelayBackoffType.Constant
    };

    private Func<CircuitBreakerPredicateArguments<object>, ValueTask<bool>> BuildCircuitBreakerPredicate(IReadOnlyCollection<string> configuredTypes)
    {
        var filters = ResolveExceptionTypes(configuredTypes);
        if (filters.Count == 0)
        {
            return args => new ValueTask<bool>(ShouldHandleDefault(args.Outcome.Exception));
        }

        return args => new ValueTask<bool>(ShouldHandleFiltered(args.Outcome.Exception, filters));
    }

    private Func<RetryPredicateArguments<object>, ValueTask<bool>> BuildRetryPredicate(IReadOnlyCollection<string> configuredTypes)
    {
        var filters = ResolveExceptionTypes(configuredTypes);
        if (filters.Count == 0)
        {
            return args => new ValueTask<bool>(ShouldHandleDefault(args.Outcome.Exception));
        }

        return args => new ValueTask<bool>(ShouldHandleFiltered(args.Outcome.Exception, filters));
    }

    private static bool ShouldHandleDefault(Exception? exception)
    {
        if (exception is null)
        {
            return false;
        }

        return exception is not OperationCanceledException;
    }

    private static bool ShouldHandleFiltered(Exception? exception, IReadOnlyList<Type> allowed)
    {
        if (exception is null)
        {
            return false;
        }

        foreach (var filter in allowed)
        {
            if (filter.IsInstanceOfType(exception))
            {
                return true;
            }
        }

        return false;
    }

    private IReadOnlyList<Type> ResolveExceptionTypes(IReadOnlyCollection<string> typeNames)
    {
        var result = new List<Type>(typeNames.Count);
        foreach (var name in typeNames)
        {
            if (string.IsNullOrWhiteSpace(name))
            {
                continue;
            }

            var type = Type.GetType(name, throwOnError: false, ignoreCase: true);
            if (type is null || !typeof(Exception).IsAssignableFrom(type))
            {
                _logger.LogWarning("Ignoring invalid retry exception type '{TypeName}'", name);
                continue;
            }

            result.Add(type);
        }

        return result.Count == 0 ? Array.Empty<Type>() : result;
    }

    private static string CreateFingerprint(ExecutionPolicyOptions options)
    {
        var hash = new HashCode();
        hash.Add(options.Retry.Enabled);
        hash.Add(options.Retry.MaxAttempts);
        hash.Add(options.Retry.InitialDelay);
        hash.Add(options.Retry.MaxDelay);
        hash.Add(options.Retry.BackoffStrategy);
        hash.Add(options.Retry.JitterFactor);
        foreach (var name in options.Retry.RetryableExceptions)
        {
            hash.Add(name, StringComparer.OrdinalIgnoreCase);
        }

        hash.Add(options.Timeout.Enabled);
        hash.Add(options.Timeout.Timeout);

        hash.Add(options.CircuitBreaker.Enabled);
        hash.Add(options.CircuitBreaker.FailureThreshold);
        hash.Add(options.CircuitBreaker.SamplingWindow);
        hash.Add(options.CircuitBreaker.BreakDuration);
        hash.Add(options.CircuitBreaker.MinimumThroughput);

        hash.Add(options.DeadLetter.Enabled);
        hash.Add(options.DeadLetter.Retention);
        hash.Add(options.DeadLetter.OperatorHint);

        return hash.ToHashCode().ToString("X");
    }

    private sealed record CachedPipeline(ResiliencePipeline Pipeline, string Fingerprint);
}
