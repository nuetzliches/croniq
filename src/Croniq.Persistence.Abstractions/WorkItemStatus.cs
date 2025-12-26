namespace Croniq.Persistence.Abstractions;

public static class WorkItemStatus
{
    public const string Queued = "queued";
    public const string Leased = "leased";
    public const string Succeeded = "succeeded";
    public const string Failed = "failed";
    public const string DeadLetter = "deadletter";
}
