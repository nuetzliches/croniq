using System.Linq;
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
        if (callerAccessor is null)
        {
            throw new ArgumentNullException(nameof(callerAccessor));
        }

        var caller = callerAccessor.Current;
        if (caller is null)
        {
            return Results.Problem(
                statusCode: StatusCodes.Status401Unauthorized,
                title: "unauthorized",
                detail: "Caller context is not available for this request.");
        }

        if (!string.Equals(caller.TenantId, tenantId, StringComparison.OrdinalIgnoreCase))
        {
            return Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "tenant-mismatch",
                detail: "API key tenant does not match the requested tenant scope.");
        }

        if (!string.IsNullOrWhiteSpace(caller.EnvironmentTag)
            && !string.Equals(caller.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
        {
            return Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "environment-mismatch",
                detail: $"API key is limited to environment '{caller.EnvironmentTag}'.");
        }

        if (requiredScopes is { Length: > 0 } && !HasAllScopes(caller, requiredScopes))
        {
            var scopeList = string.Join(", ", requiredScopes);
            return Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "insufficient-scope",
                detail: $"Scope(s) {scopeList} are required for this operation.");
        }

        return null;
    }

    private static bool HasAllScopes(ICallerContext caller, params string[] scopes)
    {
        if (scopes.Length == 0)
        {
            return true;
        }

        return scopes.All(scope => caller.Scopes.Any(assigned => string.Equals(assigned, scope, StringComparison.OrdinalIgnoreCase)));
    }
}
