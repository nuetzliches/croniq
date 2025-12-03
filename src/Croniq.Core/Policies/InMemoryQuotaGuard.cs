using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using Croniq.Core.Jobs;

namespace Croniq.Core.Policies;

/// <summary>
/// Simple in-memory quota guard; intended for single-process workers.
/// </summary>
public sealed class InMemoryQuotaGuard : IQuotaGuard
{
    private readonly ConcurrentDictionary<string, JobQuotaState> _states = new();
    private static readonly TimeSpan Window = TimeSpan.FromMinutes(1);

    public bool TryAcquire(JobKey jobKey, QuotaOptions options, DateTimeOffset now, out DateTimeOffset? retryAtUtc)
    {
        retryAtUtc = null;
        var state = _states.GetOrAdd(jobKey.Value, _ => new JobQuotaState());

        lock (state.Lock)
        {
            state.Trim(now);

            if (options.MaxParallelExecutionsPerJob > 0 && state.InFlight >= options.MaxParallelExecutionsPerJob)
            {
                retryAtUtc = now.AddSeconds(1);
                return false;
            }

            if (options.MaxTriggersPerMinute > 0 && state.Events.Count >= options.MaxTriggersPerMinute)
            {
                var oldest = state.Events.Peek();
                retryAtUtc = oldest + Window;
                return false;
            }

            state.InFlight++;
            state.Events.Enqueue(now);
            return true;
        }
    }

    public void Release(JobKey jobKey)
    {
        if (!_states.TryGetValue(jobKey.Value, out var state))
        {
            return;
        }

        lock (state.Lock)
        {
            if (state.InFlight > 0)
            {
                state.InFlight--;
            }
        }
    }

    private sealed class JobQuotaState
    {
        public Queue<DateTimeOffset> Events { get; } = new();
        public int InFlight { get; set; }
        public object Lock { get; } = new();

        public void Trim(DateTimeOffset now)
        {
            while (Events.Count > 0 && now - Events.Peek() >= Window)
            {
                Events.Dequeue();
            }
        }
    }
}
