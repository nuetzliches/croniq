namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Shared constants for execution intent fields stored with work items and leases.
/// </summary>
public static class ExecutionIntent
{
    public static class ExecutionModes
    {
        public const string Normal = "normal";
        public const string Test = "test";
    }

    public static class InvocationSources
    {
        public const string Schedule = "schedule";
        public const string Manual = "manual";
        public const string Api = "api";
        public const string WebhookIngress = "webhook-ingress";
        public const string WebhookInvoke = "webhook-invoke";
        public const string System = "system";
        public const string Replay = "replay";
        public const string Backfill = "backfill";
    }
}
