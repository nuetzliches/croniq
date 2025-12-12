namespace Croniq.Core.Execution;

/// <summary>
/// Represents the terminal outcome of a job execution.
/// </summary>
public enum JobExecutionStatus
{
    Succeeded,
    Failed,
    Canceled
}
