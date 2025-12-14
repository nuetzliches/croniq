using Croniq.Auth.Abstractions;
using Microsoft.AspNetCore.Http;

namespace Croniq.Api;

internal static class WebhookAuthorization
{
    public static class WebhookScopes
    {
        public const string Read = CroniqScopes.WebhooksRead;
        public const string Write = CroniqScopes.WebhooksWrite;
        public const string Rotate = CroniqScopes.WebhooksRotate;
        public const string DeadLetter = CroniqScopes.WebhooksDeadLetter;
    }

    internal static IResult? Ensure(
        ICallerContextAccessor callerAccessor,
        string tenantId,
        string? environment,
        params string[] requiredScopes)
    {
        return TenantGuard.EnsureTenant(callerAccessor, tenantId, environment, requiredScopes);
    }
}
