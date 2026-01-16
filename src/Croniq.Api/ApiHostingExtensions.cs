using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using System.Linq.Expressions;
using System.Reflection;
using System.Text.Json;
using System.Threading.RateLimiting;
using Croniq.Api.Models;
using Croniq.Api.Security;
using Croniq.Api.Telemetry;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Core.Security;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Croniq.Providers.Default;
using Croniq.Sdk;
using Croniq.Hosting;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.OpenApi;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static readonly ActivitySource TriggerActivitySource = new("Croniq.Api.Trigger");
    private const string CorrelationHeaderName = "X-Croniq-CorrelationId";

    public static IServiceCollection AddCroniqApiServices(this IServiceCollection services, IConfiguration configuration)
    {
        services.Configure<CroniqApiOptions>(configuration.GetSection("Croniq:Api"));
        services.Configure<WebhookIngressStreamOptions>(configuration.GetSection("Croniq:Webhooks:Ingress"));
        services.AddCroniqPlatformServices(configuration);
        services.AddSingleton<TenantRateLimitDecider>();
        services.TryAddSingleton<WebhookIngressConsumerTracker>();
        services.AddGrpc(options => options.Interceptors.Add<TenantRateLimitInterceptor>());
        return services;
    }

    public static WebApplication UseCroniqApi(this WebApplication app)
    {
        var apiOpts = app.Services.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
        var allowedNetworks = ParseAllowedNetworks(apiOpts.AllowedIpCidrs);

        if (allowedNetworks.Count > 0)
        {
            app.Use(async (context, next) =>
            {
                var remoteIp = context.Connection.RemoteIpAddress;
                if (remoteIp is null || !allowedNetworks.Any(network => network.Contains(remoteIp)))
                {
                    context.Response.StatusCode = StatusCodes.Status403Forbidden;
                    await context.Response.WriteAsync("ip not allowed");
                    return;
                }

                await next().ConfigureAwait(false);
            });
        }

        if (apiOpts.RequestsPerMinute > 0)
        {
            app.UseRateLimiter();
        }

        var anonymousPrefixes = new List<PathString>
        {
            "/health",
            "/webhooks",
            "/auth/login",
            "/auth/refresh",
            "/auth/logout",
            // When hosted behind a reverse proxy that prefixes routes with /api,
            // the application sees the full prefixed path (e.g. /api/auth/login).
            // Keep auth-less endpoints reachable in that configuration.
            "/api/health",
            "/api/webhooks",
            "/api/auth/login",
            "/api/auth/refresh",
            "/api/auth/logout"
        };

        if (apiOpts.AnonymousPathPrefixes?.Count > 0)
        {
            anonymousPrefixes.AddRange(apiOpts.AnonymousPathPrefixes.Select(p => new PathString(p)));
        }

        app.Use(async (context, next) =>
        {
            var path = context.Request.Path;
            var pathWithBase = context.Request.PathBase.Add(path);
            // Webhook ingress is authenticated by signature, not Croniq API keys.
            if (IsWebhookIngressPath(path) || IsWebhookIngressPath(pathWithBase))
            {
                await next().ConfigureAwait(false);
                return;
            }
            if (anonymousPrefixes.Any(prefix =>
                    path.StartsWithSegments(prefix, StringComparison.OrdinalIgnoreCase)
                    || pathWithBase.StartsWithSegments(prefix, StringComparison.OrdinalIgnoreCase)))
            {
                await next().ConfigureAwait(false);
                return;
            }

            var callerAccessor = context.RequestServices.GetRequiredService<ICallerContextAccessor>();
            var callerFactory = context.RequestServices.GetRequiredService<ICallerContextFactory>();
            var rateLimitDecider = context.RequestServices.GetRequiredService<TenantRateLimitDecider>();

            var authHeader = context.Request.Headers.Authorization.FirstOrDefault();
            if (!string.IsNullOrWhiteSpace(authHeader))
            {
                var bearerCaller = await callerFactory.FromBearerTokenAsync(authHeader, context.RequestAborted).ConfigureAwait(false);
                if (bearerCaller is null || !bearerCaller.IsActive)
                {
                    context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                    await context.Response.WriteAsync("invalid bearer token");
                    return;
                }

                callerAccessor.Current = bearerCaller;
                await next().ConfigureAwait(false);
                return;
            }

            var provided = context.Request.Headers["X-Croniq-Key"].FirstOrDefault();
            if (string.IsNullOrWhiteSpace(provided))
            {
                context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                await context.Response.WriteAsync("missing credentials");
                return;
            }

            var caller = await callerFactory.FromApiKeyAsync(provided, context.RequestAborted).ConfigureAwait(false);
            if (caller is null || !caller.IsActive)
            {
                context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                await context.Response.WriteAsync("invalid api key");
                return;
            }

            callerAccessor.Current = caller;

            await next().ConfigureAwait(false);
        });

        MapHealthEndpoints(app);

        if (apiOpts.Surface == CroniqApiSurface.WebhookAdminOnly)
        {
            MapWebhookEndpoints(app);
            return app;
        }

        MapCallerEndpoints(app);
        MapTenantAdminEndpoints(app);
        MapJobEndpoints(app);
        MapExecutionEndpoints(app);
        MapScheduleEndpoints(app);
        MapScheduleDeadLetterEndpoints(app);
        MapDashboardEndpoints(app);
        MapWebhookEndpoints(app);
        MapApiClientEndpoints(app);
        MapApiKeyEndpoints(app);
        MapTokenEndpoints(app);
        MapPasswordAuthEndpoints(app);
        MapExecutionLogEndpoints(app);
        MapJobTriggerEndpoints(app);
        MapWorkEndpoints(app);
        MapRunnerEndpoints(app);
        MapWorkerEndpoints(app);

        return app;
    }

    private static bool IsWebhookIngressPath(PathString path)
    {
        if (!path.HasValue)
        {
            return false;
        }

        var segments = path.Value!.Split('/', StringSplitOptions.RemoveEmptyEntries);
        return segments.Length >= 6
            && string.Equals(segments[0], "tenants", StringComparison.OrdinalIgnoreCase)
            && string.Equals(segments[2], "environments", StringComparison.OrdinalIgnoreCase)
            && string.Equals(segments[4], "webhooks", StringComparison.OrdinalIgnoreCase);
    }

    private static IReadOnlyCollection<IpNetwork> ParseAllowedNetworks(IReadOnlyCollection<string> rawCidrs)
    {
        if (rawCidrs is null || rawCidrs.Count == 0)
        {
            return Array.Empty<IpNetwork>();
        }

        var networks = new List<IpNetwork>();
        foreach (var cidr in rawCidrs)
        {
            if (!IpNetwork.TryParse(cidr, out var network, out var error) || network is null)
            {
                throw new InvalidOperationException($"Croniq:Api:AllowedIpCidrs contains invalid CIDR '{cidr}': {error ?? "invalid-cidr"}");
            }

            networks.Add(network);
        }

        return networks;
    }

    private static readonly Lazy<Func<RouteHandlerBuilder, string, string?, RouteHandlerBuilder>?> OpenApiSummaryApplier = new(CreateOpenApiSummaryApplier);

    private static RouteHandlerBuilder WithDocs(this RouteHandlerBuilder builder, string name, string summary, string? description = null)
    {
        if (builder is null)
        {
            throw new ArgumentNullException(nameof(builder));
        }

        builder.WithName(name);

        builder.WithSummary(summary);
        if (!string.IsNullOrWhiteSpace(description))
        {
            builder.WithDescription(description);
        }

        var applier = OpenApiSummaryApplier.Value;
        if (applier is not null)
        {
            return applier(builder, summary, description);
        }

        builder.WithMetadata(new EndpointDocsMetadata(summary, description));
        return builder;
    }

    private static IResult MissingEnvironment(string key = "environment")
    {
        return Results.BadRequest(new { error = "missing-environment", message = $"Query parameter '{key}' is required." });
    }

    private static string? ResolveEnvironmentTag(string? environment, ICallerContextAccessor callerContextAccessor)
    {
        if (!string.IsNullOrWhiteSpace(environment))
        {
            return environment.Trim();
        }

        var callerEnvironment = callerContextAccessor.Current?.EnvironmentTag;
        return string.IsNullOrWhiteSpace(callerEnvironment) ? null : callerEnvironment.Trim();
    }

    private static IResult? EnsureRunnerIdentity(ICallerContextAccessor callerContextAccessor, string runnerId)
    {
        if (callerContextAccessor is null)
        {
            throw new ArgumentNullException(nameof(callerContextAccessor));
        }

        var caller = callerContextAccessor.Current;
        if (caller is null)
        {
            return Results.Problem(
                statusCode: StatusCodes.Status401Unauthorized,
                title: "unauthorized",
                detail: "Caller context is not available for this request.");
        }

        if (!string.Equals(caller.CallerId, runnerId, StringComparison.OrdinalIgnoreCase))
        {
            return Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "runner-mismatch",
                detail: "RunnerId must match the authenticated caller identity.");
        }

        return null;
    }

    private static IResult? EnsureWorkerIdentity(ICallerContextAccessor callerContextAccessor, string instanceId)
    {
        if (callerContextAccessor is null)
        {
            throw new ArgumentNullException(nameof(callerContextAccessor));
        }

        var caller = callerContextAccessor.Current;
        if (caller is null)
        {
            return Results.Problem(
                statusCode: StatusCodes.Status401Unauthorized,
                title: "unauthorized",
                detail: "Caller context is not available for this request.");
        }

        if (!string.Equals(caller.CallerId, instanceId, StringComparison.OrdinalIgnoreCase))
        {
            return Results.Problem(
                statusCode: StatusCodes.Status403Forbidden,
                title: "worker-mismatch",
                detail: "InstanceId must match the authenticated caller identity.");
        }

        return null;
    }

    private static Func<RouteHandlerBuilder, string, string?, RouteHandlerBuilder>? CreateOpenApiSummaryApplier()
    {
        var operationType = Type.GetType("Microsoft.OpenApi.Models.OpenApiOperation, Microsoft.OpenApi");
        if (operationType is null)
        {
            return null;
        }

        var extensionsType = Type.GetType("Microsoft.AspNetCore.OpenApi.OpenApiRouteHandlerBuilderExtensions, Microsoft.AspNetCore.OpenApi");
        var withOpenApiMethod = extensionsType?
            .GetMethods(BindingFlags.Public | BindingFlags.Static)
            .FirstOrDefault(method =>
            {
                if (method.Name != "WithOpenApi")
                {
                    return false;
                }

                var parameters = method.GetParameters();
                if (parameters.Length != 2)
                {
                    return false;
                }

                return parameters[0].ParameterType == typeof(RouteHandlerBuilder);
            });

        if (withOpenApiMethod is null)
        {
            return null;
        }

        var applyDocsTemplate = typeof(ApiHostingExtensions)
            .GetMethod(nameof(ApplyOpenApiDocs), BindingFlags.NonPublic | BindingFlags.Static);

        if (applyDocsTemplate is null)
        {
            return null;
        }

        return (builder, summary, description) =>
        {
            var operationDelegate = CreateOpenApiOperationDelegate(operationType, applyDocsTemplate, summary, description);
            var result = withOpenApiMethod.Invoke(null, new object[] { builder, operationDelegate });
            return result as RouteHandlerBuilder ?? builder;
        };
    }

    private static Delegate CreateOpenApiOperationDelegate(Type operationType, MethodInfo applyDocsTemplate, string summary, string? description)
    {
        var applyDocsMethod = applyDocsTemplate.MakeGenericMethod(operationType);
        var operationParameter = Expression.Parameter(operationType, "operation");
        var summaryConstant = Expression.Constant(summary, typeof(string));
        var descriptionConstant = Expression.Constant(description, typeof(string));
        var call = Expression.Call(applyDocsMethod, operationParameter, summaryConstant, descriptionConstant);
        var funcType = typeof(Func<,>).MakeGenericType(operationType, operationType);
        return Expression.Lambda(funcType, call, operationParameter).Compile();
    }

    private static T ApplyOpenApiDocs<T>(T operation, string summary, string? description)
    {
        if (operation is null)
        {
            return operation!;
        }

        var type = typeof(T);
        var summaryProperty = type.GetProperty("Summary");
        summaryProperty?.SetValue(operation, summary);

        if (!string.IsNullOrWhiteSpace(description))
        {
            var descriptionProperty = type.GetProperty("Description");
            descriptionProperty?.SetValue(operation, description);
        }

        return operation!;
    }

    private sealed record EndpointDocsMetadata(string Summary, string? Description);

    private static string ResolveCallerIdentity(ICallerContextAccessor accessor)
    {
        var caller = accessor?.Current;
        return caller is null ? "cronq.api" : $"{caller.CallerType}:{caller.CallerId}";
    }

    private static string ResolveCorrelationId(HttpContext httpContext)
    {
        if (httpContext is null)
        {
            return Guid.NewGuid().ToString("N");
        }

        if (httpContext.Request.Headers.TryGetValue(CorrelationHeaderName, out var values))
        {
            var candidate = values.Count > 0 ? values[0] : null;
            if (!string.IsNullOrWhiteSpace(candidate))
            {
                return candidate!;
            }
        }

        if (!string.IsNullOrWhiteSpace(httpContext.TraceIdentifier))
        {
            return httpContext.TraceIdentifier!;
        }

        return Guid.NewGuid().ToString("N");
    }

    public static IServiceCollection AddCroniqApiRateLimiter(this IServiceCollection services)
    {
        services.AddRateLimiter(options =>
        {
            options.RejectionStatusCode = StatusCodes.Status429TooManyRequests;
            options.GlobalLimiter = PartitionedRateLimiter.Create<HttpContext, string>(context =>
            {
                var decider = context.RequestServices.GetRequiredService<TenantRateLimitDecider>();
                var callerAccessor = context.RequestServices.GetRequiredService<ICallerContextAccessor>();
                var caller = callerAccessor.Current;
                var fallbackKey = context.Request.Headers["X-Croniq-Key"].FirstOrDefault();
                var key = decider.GetPartitionId(caller, fallbackKey);
                var permits = decider.GetPermitLimit(caller);

                return RateLimitPartition.GetFixedWindowLimiter(key, _ => new FixedWindowRateLimiterOptions
                {
                    PermitLimit = permits,
                    Window = TimeSpan.FromMinutes(1),
                    QueueLimit = permits,
                    QueueProcessingOrder = QueueProcessingOrder.OldestFirst
                });
            });
        });
        return services;
    }

    public static WebApplication MapCroniqSchedulerGrpc(this WebApplication app)
    {
        app.MapGrpcService<SchedulerGrpcService>();
        return app;
    }

    public static WebApplication MapCroniqWorkerGrpc(this WebApplication app)
    {
        app.MapGrpcService<WorkerGrpcService>();
        return app;
    }

    public static WebApplication MapCroniqWebhookIngressGrpc(this WebApplication app)
    {
        app.MapGrpcService<WebhookIngressGrpcService>();
        MapWebhookIngressHttpEndpoints(app);
        return app;
    }

    private static bool TryExtractExecutionScope(string line, out string? tenantId, out string? environmentTag)
    {
        tenantId = null;
        environmentTag = null;

        if (string.IsNullOrWhiteSpace(line))
        {
            return false;
        }

        try
        {
            using var doc = JsonDocument.Parse(line);
            foreach (var property in doc.RootElement.EnumerateObject())
            {
                if (tenantId is null && string.Equals(property.Name, "tenantId", StringComparison.OrdinalIgnoreCase))
                {
                    tenantId = property.Value.GetString();
                }

                if (environmentTag is null
                    && (string.Equals(property.Name, "environmentTag", StringComparison.OrdinalIgnoreCase)
                        || string.Equals(property.Name, "environment", StringComparison.OrdinalIgnoreCase)))
                {
                    environmentTag = property.Value.GetString();
                }
            }

            return !string.IsNullOrWhiteSpace(tenantId);
        }
        catch (JsonException)
        {
            return false;
        }
    }

    private static IReadOnlyDictionary<string, string>? ToReadOnly(IDictionary<string, string>? source)
    {
        if (source is null) return null;
        if (source is IReadOnlyDictionary<string, string> ro) return ro;
        return new Dictionary<string, string>(source);
    }

    private static ApiClientResponse ToApiClientResponse(ApiClientDescriptor descriptor)
    {
        var scopes = descriptor.Scopes ?? Array.Empty<string>();
        return new ApiClientResponse(
            descriptor.ClientId,
            descriptor.TenantId,
            descriptor.Name,
            descriptor.EnvironmentTag,
            scopes,
            descriptor.IsActive,
            descriptor.ExpiresAt);
    }

    private static TenantResponse ToTenantResponse(TenantDescriptor descriptor)
    {
        return new TenantResponse(
            descriptor.TenantId,
            descriptor.Name,
            descriptor.IsActive,
            descriptor.CreatedAt);
    }

    private static CallerInfoResponse ToCallerInfoResponse(ICallerContext caller)
    {
        return new CallerInfoResponse(
            caller.TenantId,
            caller.EnvironmentTag,
            caller.CallerId,
            caller.CallerType,
            caller.Scopes,
            caller.IsActive);
    }

    private static IssueApiKeyResponse ToIssueApiKeyResponse(ApiKeyIssueResult result)
    {
        return new IssueApiKeyResponse(
            result.ClientId,
            result.TenantId,
            result.KeyId,
            result.PlaintextSecret,
            result.ExpiresAt,
            result.EnvironmentTag);
    }

    private static ScheduleResponse ToScheduleResponse(TriggerDefinition definition)
    {
        IReadOnlyDictionary<string, string>? metadata = definition.Metadata is null
            ? null
            : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

        return new ScheduleResponse(
            definition.TriggerId,
            definition.JobKey,
            definition.ScheduleExpression,
            definition.Scope.TenantId,
            definition.Scope.EnvironmentTag,
            definition.StartAtUtc,
            definition.EndAtUtc,
            definition.Enabled,
            metadata,
            definition.TimeZoneId);
    }

    private static ScheduleDeadLetterResponse ToScheduleDeadLetterResponse(JobDeadLetterEntry entry)
    {
        IReadOnlyDictionary<string, string>? metadata = entry.Metadata is null
            ? null
            : new Dictionary<string, string>(entry.Metadata, StringComparer.OrdinalIgnoreCase);

        return new ScheduleDeadLetterResponse(
            entry.Id,
            entry.TriggerId,
            entry.JobKey,
            entry.TenantId,
            entry.EnvironmentTag,
            entry.FireAtUtc,
            entry.Reason,
            entry.Payload,
            metadata,
            entry.CreatedAtUtc,
            entry.ExpiresAtUtc);
    }

    private static ExecutionResponse ToExecutionResponse(ExecutionSummary summary)
    {
        return new ExecutionResponse(
            summary.ExecutionId,
            summary.JobKey,
            summary.TenantId,
            summary.EnvironmentTag,
            summary.Kind,
            summary.Status,
            summary.FireAtUtc,
            summary.StartedAtUtc,
            summary.CompletedAtUtc,
            summary.DurationMs,
            summary.TriggerId,
            summary.InstanceId,
            summary.TraceId,
            summary.CorrelationId,
            summary.ErrorType,
            summary.ErrorMessage);
    }

    private static JobResponse ToJobResponse(JobDefinition job)
    {
        IReadOnlyDictionary<string, string>? metadata = job.Metadata is null
            ? null
            : new Dictionary<string, string>(job.Metadata, StringComparer.OrdinalIgnoreCase);

        return new JobResponse(
            job.JobKey,
            job.Namespace,
            job.Name,
            job.Variant,
            job.Description,
            metadata);
    }

    private static WebhookEndpointResponse ToWebhookResponse(WebhookEndpointDefinition definition, string? secretOverride = null)
    {
        IDictionary<string, string>? metadata = definition.Metadata is null
            ? null
            : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

        var ipRules = definition.IpRules.Count == 0
            ? Array.Empty<WebhookIpRuleResponse>()
            : definition.IpRules.Select(ToWebhookIpRuleResponse).ToArray();

        return new WebhookEndpointResponse(
            definition.HookKey,
            definition.JobKey,
            definition.Enabled,
            definition.RequireSignature,
            definition.RequestsPerMinute,
            metadata,
            ipRules,
            definition.CreatedAtUtc,
            definition.UpdatedAtUtc,
            secretOverride);
    }

    private static WebhookIpRuleResponse ToWebhookIpRuleResponse(WebhookIpRuleDefinition definition)
    {
        return new WebhookIpRuleResponse(
            definition.Id,
            definition.Cidr,
            definition.Description,
            definition.CreatedBy,
            definition.CreatedAtUtc,
            definition.UpdatedAtUtc);
    }

    private static WebhookDeadLetterResponse ToWebhookDeadLetterResponse(WebhookDeadLetterEntry entry)
    {
        IDictionary<string, string>? headers = entry.Headers is null
            ? null
            : new Dictionary<string, string>(entry.Headers, StringComparer.OrdinalIgnoreCase);

        IDictionary<string, string>? metadata = entry.Metadata is null
            ? null
            : new Dictionary<string, string>(entry.Metadata, StringComparer.OrdinalIgnoreCase);

        return new WebhookDeadLetterResponse(
            entry.Id,
            entry.HookKey,
            entry.JobKey,
            entry.TenantId,
            entry.EnvironmentTag,
            entry.Payload,
            headers,
            metadata,
            entry.FailureReason,
            entry.Attempts,
            entry.StatusCode,
            entry.ErrorDetails,
            entry.CreatedAtUtc,
            entry.LastAttemptAtUtc,
            entry.NextAttemptAtUtc,
            entry.ExpiresAtUtc);
    }

    private sealed class WebhookEndpointApiMarker
    {
    }

    private sealed class WebhookDeadLetterApiMarker
    {
    }

    private sealed class ApiKeyAdminApiMarker
    {
    }
}
