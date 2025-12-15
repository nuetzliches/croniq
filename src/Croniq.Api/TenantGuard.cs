using System.Linq;
using Croniq.Auth.Abstractions;
using Croniq.Core.Jobs;
using Microsoft.AspNetCore.Http;

namespace Croniq.Api;

internal static class TenantGuard
{
    internal static IResult? EnsureTenant(
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
                detail: "Caller tenant does not match the requested tenant scope.");
        }

        if (!string.IsNullOrWhiteSpace(caller.EnvironmentTag)
            && (string.IsNullOrWhiteSpace(environment)
                || !string.Equals(caller.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase)))
        {
            return Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "environment-mismatch",
                detail: $"Caller is limited to environment '{caller.EnvironmentTag}'.");
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

    internal static IResult? EnsureAdminScopes(
        ICallerContextAccessor callerAccessor,
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

    internal static IResult? EnsureJobScope(
        ICallerContextAccessor callerAccessor,
        JobKey jobKey,
        params string[] requiredScopes)
    {
        if (callerAccessor is null) throw new ArgumentNullException(nameof(callerAccessor));

        return EnsureTenant(callerAccessor, jobKey.TenantId, jobKey.EnvironmentTag, requiredScopes);
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
