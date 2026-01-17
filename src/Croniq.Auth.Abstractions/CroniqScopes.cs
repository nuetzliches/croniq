namespace Croniq.Auth.Abstractions;

/// <summary>Shared scope names understood by Croniq components.</summary>
public static class CroniqScopes
{
    public const string SchedulesWrite = "schedules:write";
    public const string SchedulesDeadLetter = "schedules:deadletter";
    public const string CalendarsRead = "calendars:read";
    public const string CalendarsWrite = "calendars:write";
    public const string JobsRead = "jobs:read";
    public const string JobsWrite = "jobs:write";
    public const string JobsTrigger = "jobs:trigger";
    public const string WorkPoll = "work:poll";
    public const string WorkRenew = "work:renew";
    public const string WorkAck = "work:ack";
    public const string WorkEvents = "work:events";
    public const string WorkExecute = "work:execute";
    public const string WorkersHeartbeat = "workers:heartbeat";
    public const string WorkersRead = "workers:read";
    public const string RunnersHeartbeat = "runners:heartbeat";
    public const string RunnersRead = "runners:read";
    public const string ExecutionsRead = "executions:read";
    public const string WebhooksRead = "webhooks:read";
    public const string WebhooksWrite = "webhooks:write";
    public const string WebhooksRotate = "webhooks:rotate";
    public const string WebhooksDeadLetter = "webhooks:deadletter";
    public const string WebhooksIngress = "webhooks:ingress";
    public const string ApiKeysManage = "api-keys:manage";
    public const string TenantsAdmin = "tenants:admin";
}
