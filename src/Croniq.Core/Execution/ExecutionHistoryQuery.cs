using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Query parameters used when listing execution history.
/// </summary>
public sealed class ExecutionHistoryQuery
{
    public const int DefaultLimit = 50;
    public const int MaxLimit = 200;

    public string? JobKey { get; init; }

    public ExecutionStatus? Status { get; init; }

    public DateTimeOffset? StartedAfterUtc { get; init; }

    public DateTimeOffset? StartedBeforeUtc { get; init; }

    public int Limit { get; init; } = DefaultLimit;

    public ExecutionHistoryQuery Normalize()
    {
        var limit = Limit;
        if (limit <= 0)
        {
            limit = DefaultLimit;
        }

        limit = Math.Clamp(limit, 1, MaxLimit);

        return new ExecutionHistoryQuery
        {
            JobKey = JobKey,
            Status = Status,
            StartedAfterUtc = StartedAfterUtc,
            StartedBeforeUtc = StartedBeforeUtc,
            Limit = limit
        };
    }
}
