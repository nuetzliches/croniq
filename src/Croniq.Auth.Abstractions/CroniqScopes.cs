namespace Croniq.Auth.Abstractions;

/// <summary>Shared scope names understood by Croniq components.</summary>
public static class CroniqScopes
{
    public const string SchedulesWrite = "schedules:write";
    public const string JobsRead = "jobs:read";
    public const string JobsWrite = "jobs:write";
    public const string JobsTrigger = "jobs:trigger";
    public const string ExecutionsRead = "executions:read";
    public const string WebhooksRead = "webhooks:read";
    public const string WebhooksWrite = "webhooks:write";
    public const string WebhooksRotate = "webhooks:rotate";
    public const string WebhooksDeadLetter = "webhooks:deadletter";
    public const string ApiKeysManage = "api-keys:manage";
    public const string TenantsAdmin = "tenants:admin";
}
