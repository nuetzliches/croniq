namespace Croniq.Sdk;

public enum JobState
{
    Unknown = 0,
    Waiting = 1,
    Running = 2,
    Completed = 3,
    Error = 4,
    Cancelled = 5
}
