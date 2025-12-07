using System.Diagnostics;
using System.Threading.RateLimiting;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Auth.Xtraq;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.Data.SqlServer;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Persistence.Xtraq;
using Croniq.Providers.Default;
using Croniq.Sdk;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static class ApiHostingExtensions
{
    public static IServiceCollection AddCroniqApiServices(this IServiceCollection services, IConfiguration configuration)
    {
        services.Configure<CroniqApiOptions>(configuration.GetSection("Croniq:Api"));
        services.Configure<CroniqOptions>(configuration.GetSection("Croniq:Core"));
        services.Configure<CroniqAuthOptions>(configuration.GetSection("Croniq:Auth"));
        services.Configure<CroniqPersistenceOptions>(configuration.GetSection("Croniq:Persistence"));
        services.Configure<SqlServerOptions>(configuration.GetSection("Croniq:SqlServer"));
        services.Configure<XtraqSharedOptions>(configuration.GetSection("Croniq:Xtraq"));

        services.AddCroniqCore();
        services.AddCroniqDefaultProviders();

        var authOpts = configuration.GetSection("Croniq:Auth").Get<CroniqAuthOptions>() ?? new CroniqAuthOptions();
        var persistenceOpts = configuration.GetSection("Croniq:Persistence").Get<CroniqPersistenceOptions>() ?? new CroniqPersistenceOptions();
        var sharedSqlServer = configuration.GetSection("Croniq:SqlServer").Get<SqlServerOptions>() ?? new SqlServerOptions();
        var sharedXtraq = configuration.GetSection("Croniq:Xtraq").Get<XtraqSharedOptions>() ?? new XtraqSharedOptions();
        if (string.IsNullOrWhiteSpace(sharedXtraq.ConnectionString))
        {
            sharedXtraq.ConnectionString = sharedSqlServer.ConnectionString;
        }

        // Always register InMemory JobStore
        services.AddCroniqInMemoryJobStore();

        if (string.Equals(persistenceOpts.Mode, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            var conn = ResolveConnectionString(
                persistenceOpts.SqlServer.ConnectionString,
                sharedSqlServer.ConnectionString,
                configuration);

            if (string.IsNullOrWhiteSpace(conn))
            {
                throw new InvalidOperationException("Croniq:Persistence:SqlServer:ConnectionString or Croniq:SqlServer:ConnectionString is required when Persistence.Mode = SqlServer.");
            }

            services.AddCroniqSqlServerPersistence(sqlOptions =>
            {
                sqlOptions.ConnectionString = conn;
                sqlOptions.MigrationsAssembly = persistenceOpts.SqlServer.MigrationsAssembly ?? sharedSqlServer.MigrationsAssembly;
                sqlOptions.EnableDetailedErrors = persistenceOpts.SqlServer.EnableDetailedErrors ?? sharedSqlServer.EnableDetailedErrors;
                sqlOptions.EnableSensitiveDataLogging = persistenceOpts.SqlServer.EnableSensitiveDataLogging ?? sharedSqlServer.EnableSensitiveDataLogging;
            }, persistenceOptions =>
            {
                if (persistenceOpts.SqlServer.LeaseDurationSeconds.HasValue)
                {
                    persistenceOptions.LeaseDurationSeconds = persistenceOpts.SqlServer.LeaseDurationSeconds.Value;
                }

                if (persistenceOpts.SqlServer.DeadLetterRetentionDays.HasValue)
                {
                    persistenceOptions.DeadLetterRetentionDays = persistenceOpts.SqlServer.DeadLetterRetentionDays.Value;
                }

                if (persistenceOpts.SqlServer.DeadLetterReasonMaxLength.HasValue)
                {
                    persistenceOptions.DeadLetterReasonMaxLength = persistenceOpts.SqlServer.DeadLetterReasonMaxLength.Value;
                }
            });
        }

        if (string.Equals(authOpts.Mode, "Xtraq", StringComparison.OrdinalIgnoreCase))
        {
            var conn = ResolveConnectionString(
                authOpts.Xtraq.ConnectionString,
                sharedXtraq.ConnectionString,
                configuration);

            if (string.IsNullOrWhiteSpace(conn))
            {
                throw new InvalidOperationException("Croniq:Auth:Xtraq:ConnectionString or Croniq:Xtraq:ConnectionString is required when Auth.Mode = Xtraq.");
            }

            services.AddXtraqDbContext(options =>
            {
                options.ConnectionString = conn;
            });
            services.AddCroniqAuthXtraq();
        }
        else
        {
            var apiKey = authOpts.InMemory.ApiKey;
            if (string.IsNullOrWhiteSpace(apiKey))
            {
                throw new InvalidOperationException("Croniq:Auth:InMemory:ApiKey must be set when Auth.Mode = InMemory.");
            }

            services.AddCroniqAuthCore(options =>
            {
                options.ApiKeys.Add(new ApiKeySeed(
                    KeyId: "default",
                    Secret: apiKey,
                    TenantId: authOpts.InMemory.TenantId,
                    EnvironmentTag: authOpts.InMemory.EnvironmentTag,
                    Scopes: new[] { "schedules:write", "jobs:trigger" },
                    ClientId: "default"));
            });
        }

        services.TryAddScoped<ICallerContextAccessor, CallerContextAccessor>();
        services.TryAddScoped<ICallerContextFactory, CallerContextFactory>();

        return services;
    }

    public static WebApplication UseCroniqApi(this WebApplication app)
    {
        var apiOpts = app.Services.GetRequiredService<IOptions<CroniqApiOptions>>().Value;

        if (apiOpts.RequestsPerMinute > 0)
        {
            app.UseRateLimiter();
        }

        app.Use(async (context, next) =>
        {
            if (context.Request.Path.StartsWithSegments("/health", StringComparison.OrdinalIgnoreCase))
            {
                await next().ConfigureAwait(false);
                return;
            }

            var options = context.RequestServices.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
            var callerAccessor = context.RequestServices.GetRequiredService<ICallerContextAccessor>();
            var callerFactory = context.RequestServices.GetRequiredService<ICallerContextFactory>();

            var provided = context.Request.Headers["X-Croniq-Key"].FirstOrDefault();
            if (string.IsNullOrWhiteSpace(provided))
            {
                context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                await context.Response.WriteAsync("missing api key");
                return;
            }

            if (!string.IsNullOrWhiteSpace(provided))
            {
                var caller = await callerFactory.FromApiKeyAsync(provided, context.RequestAborted).ConfigureAwait(false);
                if (caller is null || !caller.IsActive)
                {
                    context.Response.StatusCode = StatusCodes.Status401Unauthorized;
                    await context.Response.WriteAsync("invalid api key");
                    return;
                }
                callerAccessor.Current = caller;
            }

            await next().ConfigureAwait(false);
        });

        app.MapGet("/health", () => Results.Ok(new { status = "ok" }));
        app.MapGet("/health/persistence", async (IServiceProvider sp, CancellationToken ct) =>
        {
            var provider = sp.GetService<IJobPersistenceProvider>();
            var providerName = provider?.GetType().FullName ?? "unknown";

            var health = sp.GetService<IPersistenceHealth>();
            if (health is null)
            {
                return Results.Ok(new { status = "ok", provider = providerName, note = "no-db-provider-configured" });
            }

            try
            {
                var result = await health.CheckAsync(ct).ConfigureAwait(false);
                if (result.IsHealthy)
                {
                    return Results.Ok(new { status = "ok", provider = providerName, db = "reachable" });
                }

                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unhealthy", detail: result.Detail);
            }
            catch (Exception ex)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unreachable", detail: ex.Message);
            }
        });

        app.MapPost("/schedules", async (
            UpsertScheduleRequest request,
            IJobPersistenceProvider store,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(request.JobKey) || string.IsNullOrWhiteSpace(request.CronExpression))
            {
                return Results.BadRequest(new { error = "invalid-request", message = "JobKey and CronExpression are required." });
            }

            var parts = ParseJobKey(request.JobKey);
            var triggerId = string.IsNullOrWhiteSpace(request.TriggerId)
                ? $"{request.JobKey}:{request.CronExpression}"
                : request.TriggerId;

            var scope = new PartitionScope(parts.TenantId, parts.EnvironmentTag);

            var metadata = ToReadOnly(request.Metadata);
            var job = new JobDefinition(
                request.JobKey,
                parts.NamespaceSegment,
                parts.JobName,
                parts.Variant,
                request.Description,
                metadata);

            var trigger = new TriggerDefinition(
                triggerId,
                request.JobKey,
                request.CronExpression,
                scope,
                request.StartAtUtc,
                request.EndAtUtc,
                request.Enabled,
                metadata);

            await store.UpsertJobAsync(job, cancellationToken).ConfigureAwait(false);
            await store.UpsertTriggerAsync(trigger, cancellationToken).ConfigureAwait(false);

            return Results.Created($"/schedules/{trigger.TriggerId}", new { trigger.TriggerId, trigger.JobKey, trigger.ScheduleExpression });
        });

        app.MapPost("/jobs/trigger", async (
            TriggerJobRequest request,
            IJobRegistry registry,
            IJobExecutionPipeline pipeline,
            IPolicyResolver policyResolver,
            CancellationToken cancellationToken) =>
        {
            if (!JobKey.TryParse(request.JobKey, out var jobKey) || !registry.TryGet(jobKey, out var descriptor))
            {
                return Results.NotFound(new { error = "job-not-registered", request.JobKey });
            }

            var metadata = ToReadOnly(request.Metadata) ?? new Dictionary<string, string>();
            var activitySource = new ActivitySource("Croniq.Api.Trigger");
            var executionOptions = policyResolver.ResolveExecution(jobKey);
            var execRequest = new JobExecutionRequest(jobKey, descriptor, executionOptions, metadata, activitySource);

            await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
            return Results.Accepted(value: new { status = "triggered", request.JobKey });
        });

        return app;
    }

    public static IServiceCollection AddCroniqApiRateLimiter(this IServiceCollection services)
    {
        services.AddRateLimiter(options =>
        {
            options.RejectionStatusCode = StatusCodes.Status429TooManyRequests;
            options.GlobalLimiter = PartitionedRateLimiter.Create<HttpContext, string>(context =>
            {
                var configured = context.RequestServices.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
                var key = context.Request.Headers["X-Croniq-Key"].FirstOrDefault() ?? "anonymous";
                var permits = Math.Max(1, configured.RequestsPerMinute);

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

    private static (string TenantId, string EnvironmentTag, string NamespaceSegment, string JobName, string? Variant) ParseJobKey(string jobKey)
    {
        if (!JobKey.TryParse(jobKey, out var parsed))
        {
            throw new ArgumentException($"Invalid JobKey format: {jobKey}", nameof(jobKey));
        }

        return (parsed.TenantId, parsed.EnvironmentTag, parsed.NamespaceSegment, parsed.JobName, parsed.Variant);
    }

    private static IReadOnlyDictionary<string, string>? ToReadOnly(IDictionary<string, string>? source)
    {
        if (source is null) return null;
        if (source is IReadOnlyDictionary<string, string> ro) return ro;
        return new Dictionary<string, string>(source);
    }

    private static string? ResolveConnectionString(string? domainSpecific, string? shared, IConfiguration configuration)
    {
        return domainSpecific
            ?? shared
            ?? configuration.GetConnectionString("CroniqSqlServer")
            ?? configuration.GetConnectionString("CroniqXtraq")
            ?? configuration.GetConnectionString("Croniq")
            ?? configuration.GetConnectionString("DefaultConnection");
    }
}
