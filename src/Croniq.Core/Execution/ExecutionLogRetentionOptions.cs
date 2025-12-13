using System;

namespace Croniq.Core.Execution;

public sealed class ExecutionLogRetentionOptions
{
    public int RetentionDays { get; set; } = 7;

    public TimeSpan SweepInterval { get; set; } = TimeSpan.FromHours(1);
}
