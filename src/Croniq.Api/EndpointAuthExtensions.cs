using System;
using System.Linq;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Core.Jobs;
using Microsoft.AspNetCore.Http;

namespace Croniq.Api;

internal static class EndpointAuthExtensions
{
    internal static RouteHandlerBuilder RequireCroniqCaller(this RouteHandlerBuilder builder)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.Caller));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqCallerEndpointFilter());
    }

    /// <summary>
    /// Marks an endpoint as callable even when the authenticated user must change their password.
    /// </summary>
    internal static RouteHandlerBuilder AllowPasswordChangeRequired(this RouteHandlerBuilder builder)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));

        builder.WithMetadata(new PasswordChangeRequiredBypassMetadata());
        return builder;
    }

    internal static RouteHandlerBuilder RequireCroniqAdminScopes(this RouteHandlerBuilder builder, params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.AdminScopes, requiredScopes ?? Array.Empty<string>()));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqAdminScopesEndpointFilter(requiredScopes ?? Array.Empty<string>()));
    }

    internal static RouteHandlerBuilder RequireCroniqTenantScope(
        this RouteHandlerBuilder builder,
        params string[] requiredScopes)
        => builder.RequireCroniqTenantScope(requireEnvironment: true, requiredScopes);

    internal static RouteHandlerBuilder RequireCroniqTenantScope(
        this RouteHandlerBuilder builder,
        bool requireEnvironment,
        params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.TenantScope, requiredScopes ?? Array.Empty<string>(), requireEnvironment));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqTenantScopeEndpointFilter(
            tenantRouteKey: "tenantId",
            environmentQueryKey: "environment",
            requireEnvironment,
            requiredScopes ?? Array.Empty<string>()));
    }

    internal static RouteHandlerBuilder RequireCroniqTenantScopeFromRoute(
        this RouteHandlerBuilder builder,
        string environmentRouteKey,
        params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));
        _ = environmentRouteKey ?? throw new ArgumentNullException(nameof(environmentRouteKey));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.TenantScope, requiredScopes ?? Array.Empty<string>(), RequireEnvironment: true));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqTenantScopeFromRouteEndpointFilter(
            tenantRouteKey: "tenantId",
            environmentRouteKey,
            requireEnvironment: true,
            requiredScopes ?? Array.Empty<string>()));
    }

    internal static RouteHandlerBuilder RequireCroniqTenantScopeFromBody<TRequest>(
        this RouteHandlerBuilder builder,
        Func<TRequest, string?> environmentSelector,
        params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));
        _ = environmentSelector ?? throw new ArgumentNullException(nameof(environmentSelector));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.TenantScopeFromBody, requiredScopes ?? Array.Empty<string>(), false));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqTenantScopeFromBodyEndpointFilter<TRequest>(
            tenantRouteKey: "tenantId",
            environmentQueryKey: null,
            requireEnvironment: false,
            environmentSelector,
            requiredScopes ?? Array.Empty<string>()));
    }

    internal static RouteHandlerBuilder RequireCroniqTenantScopeFromBodyOrQuery<TRequest>(
        this RouteHandlerBuilder builder,
        Func<TRequest, string?> environmentSelector,
        bool requireEnvironment,
        params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));
        _ = environmentSelector ?? throw new ArgumentNullException(nameof(environmentSelector));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.TenantScopeFromBodyOrQuery, requiredScopes ?? Array.Empty<string>(), requireEnvironment));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqTenantScopeFromBodyEndpointFilter<TRequest>(
            tenantRouteKey: "tenantId",
            environmentQueryKey: "environment",
            requireEnvironment,
            environmentSelector,
            requiredScopes ?? Array.Empty<string>()));
    }

    internal static RouteHandlerBuilder RequireCroniqTokenIssueScopes(this RouteHandlerBuilder builder, params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.TokenIssue, requiredScopes ?? Array.Empty<string>(), false));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqTokenIssueEndpointFilter(requiredScopes ?? Array.Empty<string>()));
    }

    internal static RouteHandlerBuilder RequireCroniqJobScopeFromBody<TRequest>(
        this RouteHandlerBuilder builder,
        Func<TRequest, string?> jobKeySelector,
        params string[] requiredScopes)
    {
        _ = builder ?? throw new ArgumentNullException(nameof(builder));
        _ = jobKeySelector ?? throw new ArgumentNullException(nameof(jobKeySelector));

        builder.WithMetadata(new CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind.JobScopeFromBody, requiredScopes ?? Array.Empty<string>(), false));

        builder.AddEndpointFilter(new PasswordChangeRequiredEndpointFilter());
        return builder.AddEndpointFilter(new CroniqJobScopeFromBodyEndpointFilter<TRequest>(jobKeySelector, requiredScopes ?? Array.Empty<string>()));
    }

    internal enum CroniqAuthGuardKind
    {
        Caller = 0,
        AdminScopes = 1,
        TenantScope = 2,
        TenantScopeFromBody = 3,
        TenantScopeFromBodyOrQuery = 4,
        TokenIssue = 5,
        JobScopeFromBody = 6,
        TenantScopeDerived = 7,
        JobScopeDerived = 8
    }

    internal interface ICroniqAuthEndpointGuardMetadata
    {
        CroniqAuthGuardKind Kind { get; }
        bool RequireEnvironment { get; }
        string[] RequiredScopes { get; }
    }

    private sealed record PasswordChangeRequiredBypassMetadata;

    private sealed class PasswordChangeRequiredEndpointFilter : IEndpointFilter
    {
        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var httpContext = context.HttpContext;

            var endpoint = httpContext.GetEndpoint();
            if (endpoint?.Metadata.GetMetadata<PasswordChangeRequiredBypassMetadata>() is not null)
            {
                return next(context);
            }

            var claim = httpContext.User?.FindFirst(CroniqClaimNames.PasswordChangeRequired);
            if (claim is null)
            {
                return next(context);
            }

            var raw = claim.Value;
            var required = string.Equals(raw, "true", StringComparison.OrdinalIgnoreCase)
                || string.Equals(raw, "1", StringComparison.OrdinalIgnoreCase);

            if (!required)
            {
                return next(context);
            }

            // When password change is required, block access to protected endpoints.
            // TODO (2FA): When adding MFA/2FA, ensure this does not block 2FA enrollment/verification endpoints.
            // Those should be explicitly allowed similar to /auth/change-password.
            return ValueTask.FromResult<object?>(Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "password-change-required",
                detail: "Password change required before accessing this endpoint."));
        }
    }

    internal sealed record CroniqAuthEndpointGuardMetadata(
        CroniqAuthGuardKind Kind,
        string[] RequiredScopes,
        bool RequireEnvironment) : ICroniqAuthEndpointGuardMetadata
    {
        public CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind kind)
            : this(kind, Array.Empty<string>(), false)
        {
        }

        public CroniqAuthEndpointGuardMetadata(CroniqAuthGuardKind kind, string[] requiredScopes)
            : this(kind, requiredScopes ?? Array.Empty<string>(), false)
        {
        }
    }

    private sealed class CroniqCallerEndpointFilter : IEndpointFilter
    {
        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var callerAccessor = context.HttpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            var failure = callerAccessor is null
                ? Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered.")
                : TenantGuard.EnsureAdminScopes(callerAccessor);

            return failure is not null
                ? ValueTask.FromResult<object?>(failure)
                : next(context);
        }
    }

    private sealed class CroniqAdminScopesEndpointFilter : IEndpointFilter
    {
        private readonly string[] _requiredScopes;

        public CroniqAdminScopesEndpointFilter(string[] requiredScopes)
        {
            _requiredScopes = requiredScopes;
        }

        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var callerAccessor = context.HttpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            var failure = callerAccessor is null
                ? Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered.")
                : TenantGuard.EnsureAdminScopes(callerAccessor, _requiredScopes);

            return failure is not null
                ? ValueTask.FromResult<object?>(failure)
                : next(context);
        }
    }

    private sealed class CroniqTenantScopeEndpointFilter : IEndpointFilter
    {
        private readonly string _tenantRouteKey;
        private readonly string _environmentQueryKey;
        private readonly bool _requireEnvironment;
        private readonly string[] _requiredScopes;

        public CroniqTenantScopeEndpointFilter(
            string tenantRouteKey,
            string environmentQueryKey,
            bool requireEnvironment,
            string[] requiredScopes)
        {
            _tenantRouteKey = tenantRouteKey;
            _environmentQueryKey = environmentQueryKey;
            _requireEnvironment = requireEnvironment;
            _requiredScopes = requiredScopes;
        }

        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var httpContext = context.HttpContext;

            var tenantId = httpContext.Request.RouteValues.TryGetValue(_tenantRouteKey, out var tenantValue)
                ? tenantValue?.ToString()
                : null;

            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "missing-tenant", message = $"Route parameter '{_tenantRouteKey}' is required." }));
            }

            var environment = httpContext.Request.Query.TryGetValue(_environmentQueryKey, out var envValues)
                ? envValues.FirstOrDefault()
                : null;
            var callerAccessor = httpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            if (callerAccessor is null)
            {
                return ValueTask.FromResult<object?>(Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered."));
            }

            if (_requireEnvironment && string.IsNullOrWhiteSpace(environment))
            {
                environment = callerAccessor.Current?.EnvironmentTag;
                if (string.IsNullOrWhiteSpace(environment))
                {
                    return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "missing-environment", message = $"Query parameter '{_environmentQueryKey}' is required." }));
                }
            }

            var failure = TenantGuard.EnsureTenant(callerAccessor, tenantId, environment, _requiredScopes);
            return failure is not null
                ? ValueTask.FromResult<object?>(failure)
                : next(context);
        }
    }

    private sealed class CroniqTenantScopeFromRouteEndpointFilter : IEndpointFilter
    {
        private readonly string _tenantRouteKey;
        private readonly string _environmentRouteKey;
        private readonly bool _requireEnvironment;
        private readonly string[] _requiredScopes;

        public CroniqTenantScopeFromRouteEndpointFilter(
            string tenantRouteKey,
            string environmentRouteKey,
            bool requireEnvironment,
            string[] requiredScopes)
        {
            _tenantRouteKey = tenantRouteKey;
            _environmentRouteKey = environmentRouteKey;
            _requireEnvironment = requireEnvironment;
            _requiredScopes = requiredScopes;
        }

        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var httpContext = context.HttpContext;

            var tenantId = httpContext.Request.RouteValues.TryGetValue(_tenantRouteKey, out var tenantValue)
                ? tenantValue?.ToString()
                : null;

            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "missing-tenant", message = $"Route parameter '{_tenantRouteKey}' is required." }));
            }

            var environment = httpContext.Request.RouteValues.TryGetValue(_environmentRouteKey, out var envValue)
                ? envValue?.ToString()
                : null;

            var callerAccessor = httpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            if (callerAccessor is null)
            {
                return ValueTask.FromResult<object?>(Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered."));
            }

            if (_requireEnvironment && string.IsNullOrWhiteSpace(environment))
            {
                environment = callerAccessor.Current?.EnvironmentTag;
                if (string.IsNullOrWhiteSpace(environment))
                {
                    return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "missing-environment", message = $"Route parameter '{_environmentRouteKey}' is required." }));
                }
            }

            var failure = TenantGuard.EnsureTenant(callerAccessor, tenantId, environment, _requiredScopes);
            return failure is not null
                ? ValueTask.FromResult<object?>(failure)
                : next(context);
        }
    }

    private sealed class CroniqTenantScopeFromBodyEndpointFilter<TRequest> : IEndpointFilter
    {
        private readonly string _tenantRouteKey;
        private readonly string? _environmentQueryKey;
        private readonly bool _requireEnvironment;
        private readonly Func<TRequest, string?> _environmentSelector;
        private readonly string[] _requiredScopes;

        public CroniqTenantScopeFromBodyEndpointFilter(
            string tenantRouteKey,
            string? environmentQueryKey,
            bool requireEnvironment,
            Func<TRequest, string?> environmentSelector,
            string[] requiredScopes)
        {
            _tenantRouteKey = tenantRouteKey;
            _environmentQueryKey = environmentQueryKey;
            _requireEnvironment = requireEnvironment;
            _environmentSelector = environmentSelector;
            _requiredScopes = requiredScopes;
        }

        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var httpContext = context.HttpContext;

            var tenantId = httpContext.Request.RouteValues.TryGetValue(_tenantRouteKey, out var tenantValue)
                ? tenantValue?.ToString()
                : null;

            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "missing-tenant", message = $"Route parameter '{_tenantRouteKey}' is required." }));
            }

            var request = context.Arguments.OfType<TRequest>().FirstOrDefault();
            var environment = request is null ? null : _environmentSelector(request);

            if (string.IsNullOrWhiteSpace(environment)
                && !string.IsNullOrWhiteSpace(_environmentQueryKey)
                && httpContext.Request.Query.TryGetValue(_environmentQueryKey, out var envValues))
            {
                environment = envValues.FirstOrDefault();
            }
            var callerAccessor = httpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            if (callerAccessor is null)
            {
                return ValueTask.FromResult<object?>(Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered."));
            }

            if (_requireEnvironment && string.IsNullOrWhiteSpace(environment))
            {
                environment = callerAccessor.Current?.EnvironmentTag;
                if (string.IsNullOrWhiteSpace(environment))
                {
                    var key = string.IsNullOrWhiteSpace(_environmentQueryKey) ? "environment" : _environmentQueryKey;
                    return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "missing-environment", message = $"Query parameter '{key}' is required." }));
                }
            }

            var failure = TenantGuard.EnsureTenant(callerAccessor, tenantId, environment, _requiredScopes);
            return failure is not null
                ? ValueTask.FromResult<object?>(failure)
                : next(context);
        }
    }

    private sealed class CroniqTokenIssueEndpointFilter : IEndpointFilter
    {
        private readonly string[] _requiredScopes;

        public CroniqTokenIssueEndpointFilter(string[] requiredScopes)
        {
            _requiredScopes = requiredScopes;
        }

        public async ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var httpContext = context.HttpContext;

            var tenantId = httpContext.Request.RouteValues.TryGetValue("tenantId", out var tenantValue)
                ? tenantValue?.ToString()
                : null;

            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "Route parameter 'tenantId' is required." });
            }

            var routeClientId = httpContext.Request.RouteValues.TryGetValue("clientId", out var clientValue)
                ? clientValue?.ToString()
                : null;

            var request = context.Arguments.OfType<IssueTokenRequest>().FirstOrDefault();
            var clientId = routeClientId ?? request?.ClientId;

            if (string.IsNullOrWhiteSpace(clientId))
            {
                return await next(context).ConfigureAwait(false);
            }

            var apiKeyStore = httpContext.RequestServices.GetService(typeof(IApiKeyStore)) as IApiKeyStore;
            if (apiKeyStore is null)
            {
                return await next(context).ConfigureAwait(false);
            }

            var client = await apiKeyStore.GetClientAsync(tenantId, clientId, httpContext.RequestAborted).ConfigureAwait(false);
            if (client is null)
            {
                return await next(context).ConfigureAwait(false);
            }

            httpContext.Items[typeof(ApiClientDescriptor)] = client;

            if (!client.IsActive)
            {
                return await next(context).ConfigureAwait(false);
            }

            var environment = httpContext.Request.Query.TryGetValue("environment", out var envValues)
                ? envValues.FirstOrDefault()
                : null;

            if (!string.IsNullOrWhiteSpace(environment)
                && !string.IsNullOrWhiteSpace(client.EnvironmentTag)
                && !string.Equals(client.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
            {
                return await next(context).ConfigureAwait(false);
            }

            var guardEnvironment = environment ?? client.EnvironmentTag;
            var callerAccessor = httpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            if (callerAccessor is null)
            {
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered.");
            }

            var failure = TenantGuard.EnsureTenant(callerAccessor, tenantId, guardEnvironment, _requiredScopes);
            return failure is not null
                ? failure
                : await next(context).ConfigureAwait(false);
        }
    }

    private sealed class CroniqJobScopeFromBodyEndpointFilter<TRequest> : IEndpointFilter
    {
        private readonly Func<TRequest, string?> _jobKeySelector;
        private readonly string[] _requiredScopes;

        public CroniqJobScopeFromBodyEndpointFilter(Func<TRequest, string?> jobKeySelector, string[] requiredScopes)
        {
            _jobKeySelector = jobKeySelector;
            _requiredScopes = requiredScopes;
        }

        public ValueTask<object?> InvokeAsync(EndpointFilterInvocationContext context, EndpointFilterDelegate next)
        {
            var httpContext = context.HttpContext;
            var request = context.Arguments.OfType<TRequest>().FirstOrDefault();
            var jobKeyRaw = request is null ? null : _jobKeySelector(request);

            if (string.IsNullOrWhiteSpace(jobKeyRaw) || !JobKey.TryParse(jobKeyRaw, out var jobKey))
            {
                return ValueTask.FromResult<object?>(Results.BadRequest(new { error = "invalid-job-key", message = "JobKey must follow the Croniq format." }));
            }

            var callerAccessor = httpContext.RequestServices.GetService(typeof(ICallerContextAccessor)) as ICallerContextAccessor;
            if (callerAccessor is null)
            {
                return ValueTask.FromResult<object?>(Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "caller-context-missing", detail: "Caller context accessor not registered."));
            }

            httpContext.Items[typeof(JobKey)] = jobKey;

            var failure = TenantGuard.EnsureAdminScopes(callerAccessor, _requiredScopes);
            return failure is not null
                ? ValueTask.FromResult<object?>(failure)
                : next(context);
        }
    }
}
