using System.Collections.Concurrent;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Observability;
using Croniq.Core.Policies;
using Croniq.Core.Security;
using Croniq.Data.Postgres;
using Croniq.Data.SqlServer;
using Croniq.Hosting;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Postgres;
using Croniq.Persistence.SqlServer;
using Croniq.Webhooks.InMemory;
using Croniq.Webhooks.Options;
using Croniq.Webhooks.Relay;
using Croniq.Webhooks.Remote;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.Caching.Memory;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Options;
using System.Threading.RateLimiting;

namespace Croniq.Webhooks;

public static class WebhookHostingExtensions
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Webhooks.Ingress");
    private static readonly ConcurrentDictionary<string, byte> UnsignedWarningCache = new(StringComparer.OrdinalIgnoreCase);
    private const string WebhookOptionsSectionName = "Croniq:Webhooks";
    private const string DataProtectionSectionName = "Croniq:Security:DataProtection";
    private const string DefaultDataProtectionAppName = "Croniq";
    private static string BuildEndpointCacheKey(PartitionScope scope, string hookKey)
        => $"webhook:endpoint:{scope.TenantId.ToLowerInvariant()}:{scope.EnvironmentTag.ToLowerInvariant()}:{hookKey.ToLowerInvariant()}";

    public static IServiceCollection AddCroniqWebhookPersistence(this IServiceCollection services, IConfiguration configuration, string sectionName = WebhookOptionsSectionName)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configuration is null) throw new ArgumentNullException(nameof(configuration));

        var options = configuration.GetSection(sectionName).Get<CroniqWebhookOptions>() ?? new CroniqWebhookOptions();
        services.Configure<CroniqWebhookOptions>(configuration.GetSection(sectionName));

        return options.Mode switch
        {
            WebhookPersistenceMode.SqlServer => ConfigureWebhookSqlServerPersistence(services, configuration, options),
            WebhookPersistenceMode.Postgres => ConfigureWebhookPostgresPersistence(services, configuration, options),
            WebhookPersistenceMode.InMemory => ConfigureWebhookInMemoryPersistence(services),
            WebhookPersistenceMode.Remote => ConfigureWebhookRemotePersistence(services, options),
            _ => throw new InvalidOperationException($"Unsupported Croniq:Webhooks:Mode '{options.Mode}'. Supported values: InMemory, SqlServer, Postgres, Remote."),
        };
    }

    private static IServiceCollection ConfigureWebhookInMemoryPersistence(IServiceCollection services)
    {
        services.TryAddSingleton<InMemoryWebhookPersistenceProvider>();
        services.TryAddSingleton<IWebhookPersistenceProvider>(sp => sp.GetRequiredService<InMemoryWebhookPersistenceProvider>());
        services.TryAddSingleton<InMemoryWebhookDeadLetterStore>();
        services.TryAddSingleton<IWebhookDeadLetterStore>(sp => sp.GetRequiredService<InMemoryWebhookDeadLetterStore>());
        return services;
    }

    private static IServiceCollection ConfigureWebhookSqlServerPersistence(IServiceCollection services, IConfiguration configuration, CroniqWebhookOptions options)
    {
        var sharedSql = configuration.GetSection("Croniq:SqlServer").Get<SqlServerOptions>() ?? new SqlServerOptions();
        var connectionString = options.SqlServer.ConnectionString ?? sharedSql.ConnectionString ?? ResolveSqlServerConnectionString(configuration);

        if (string.IsNullOrWhiteSpace(connectionString))
        {
            throw new InvalidOperationException("Croniq:Webhooks:SqlServer:ConnectionString or Croniq:SqlServer:ConnectionString must be provided when Croniq:Webhooks:Mode = SqlServer.");
        }

        ConfigureDataProtection(services, configuration);
        services.AddCroniqSqlServerDbContext(sqlOptions =>
        {
            sqlOptions.ConnectionString = connectionString;
            sqlOptions.MigrationsAssembly = options.SqlServer.MigrationsAssembly ?? sharedSql.MigrationsAssembly;
            sqlOptions.EnableDetailedErrors = options.SqlServer.EnableDetailedErrors ?? sharedSql.EnableDetailedErrors;
            sqlOptions.EnableSensitiveDataLogging = options.SqlServer.EnableSensitiveDataLogging ?? sharedSql.EnableSensitiveDataLogging;
            sqlOptions.CommandTimeoutSeconds = options.SqlServer.CommandTimeoutSeconds ?? sharedSql.CommandTimeoutSeconds;
        });

        services.TryAddSingleton<IWebhookPersistenceProvider, SqlServerWebhookPersistenceProvider>();
        services.TryAddSingleton<IWebhookDeadLetterStore, SqlServerWebhookDeadLetterStore>();
        services.TryAddSingleton<IWebhookIngressEventStore, SqlServerWebhookIngressEventStore>();
        services.TryAddSingleton<IWebhookEndpointChangefeed, SqlServerWebhookEndpointChangefeed>();
        return services;
    }

    private static IServiceCollection ConfigureWebhookPostgresPersistence(IServiceCollection services, IConfiguration configuration, CroniqWebhookOptions options)
    {
        var sharedPostgres = configuration.GetSection("Croniq:Postgres").Get<PostgresOptions>() ?? new PostgresOptions();
        var connectionString = options.Postgres.ConnectionString ?? sharedPostgres.ConnectionString ?? ResolvePostgresConnectionString(configuration);

        if (string.IsNullOrWhiteSpace(connectionString))
        {
            throw new InvalidOperationException("Croniq:Webhooks:Postgres:ConnectionString or Croniq:Postgres:ConnectionString must be provided when Croniq:Webhooks:Mode = Postgres.");
        }

        ConfigureDataProtection(services, configuration);
        services.AddCroniqPostgresDbContext(pgOptions =>
        {
            pgOptions.ConnectionString = connectionString;
            pgOptions.MigrationsAssembly = options.Postgres.MigrationsAssembly ?? sharedPostgres.MigrationsAssembly;
            pgOptions.EnableDetailedErrors = options.Postgres.EnableDetailedErrors ?? sharedPostgres.EnableDetailedErrors;
            pgOptions.EnableSensitiveDataLogging = options.Postgres.EnableSensitiveDataLogging ?? sharedPostgres.EnableSensitiveDataLogging;
            pgOptions.CommandTimeoutSeconds = options.Postgres.CommandTimeoutSeconds ?? sharedPostgres.CommandTimeoutSeconds;
            pgOptions.SearchPath = sharedPostgres.SearchPath;
        });

        services.TryAddSingleton<IWebhookPersistenceProvider, PostgresWebhookPersistenceProvider>();
        services.TryAddSingleton<IWebhookDeadLetterStore, PostgresWebhookDeadLetterStore>();
        services.TryAddSingleton<IWebhookIngressEventStore, PostgresWebhookIngressEventStore>();
        services.TryAddSingleton<IWebhookEndpointChangefeed, PostgresWebhookEndpointChangefeed>();
        return services;
    }

    private static void ConfigureDataProtection(IServiceCollection services, IConfiguration configuration)
    {
        var builder = services.AddDataProtection();
        var section = configuration.GetSection(DataProtectionSectionName);
        var keyRingPath = section.GetValue<string>("KeyRingPath");
        var applicationName = section.GetValue<string>("ApplicationName");

        if (!string.IsNullOrWhiteSpace(keyRingPath))
        {
            builder.PersistKeysToFileSystem(new DirectoryInfo(keyRingPath));
        }

        var resolvedName = string.IsNullOrWhiteSpace(applicationName)
            ? DefaultDataProtectionAppName
            : applicationName;

        services.PostConfigure<DataProtectionOptions>(options =>
        {
            if (string.IsNullOrWhiteSpace(options.ApplicationDiscriminator))
            {
                options.ApplicationDiscriminator = resolvedName;
            }
        });
    }

    private static IServiceCollection ConfigureWebhookRemotePersistence(IServiceCollection services, CroniqWebhookOptions options)
    {
        var remote = options.Remote ?? new WebhookRemoteOptions();
        if (string.IsNullOrWhiteSpace(remote.BaseUrl))
        {
            throw new InvalidOperationException("Croniq:Webhooks:Remote:BaseUrl must be provided when Croniq:Webhooks:Mode = Remote.");
        }

        if (!Uri.TryCreate(remote.BaseUrl.Trim(), UriKind.Absolute, out var baseUri))
        {
            throw new InvalidOperationException("Croniq:Webhooks:Remote:BaseUrl must be a valid absolute URI when Croniq:Webhooks:Mode = Remote.");
        }

        if (!string.IsNullOrWhiteSpace(remote.ApiKey))
        {
            remote.ApiKey = remote.ApiKey.Trim();
        }

        if (string.IsNullOrWhiteSpace(remote.ApiKey))
        {
            throw new InvalidOperationException("Croniq:Webhooks:Remote:ApiKey must be provided when Croniq:Webhooks:Mode = Remote.");
        }

        var normalizedBase = baseUri.AbsoluteUri.EndsWith("/", StringComparison.Ordinal)
            ? baseUri
            : new Uri(baseUri.AbsoluteUri + "/", UriKind.Absolute);
        var timeoutSeconds = Math.Max(1, remote.TimeoutSeconds);

        var allowInvalidCertificate = remote.AllowInvalidServerCertificate && normalizedBase.Scheme == Uri.UriSchemeHttps;

        RemoveWebhookPersistenceRegistrations(services);

        var remoteClientBuilder = services.AddHttpClient<RemoteWebhookClient>(client =>
        {
            client.BaseAddress = normalizedBase;
            client.Timeout = TimeSpan.FromSeconds(timeoutSeconds);
            client.DefaultRequestHeaders.Add("X-Croniq-Key", remote.ApiKey);
        });

        if (allowInvalidCertificate)
        {
            remoteClientBuilder.ConfigurePrimaryHttpMessageHandler(() => new HttpClientHandler
            {
                ServerCertificateCustomValidationCallback = HttpClientHandler.DangerousAcceptAnyServerCertificateValidator
            });
        }

        services.AddSingleton<IWebhookPersistenceProvider, RemoteWebhookPersistenceProvider>();
        services.AddSingleton<IWebhookDeadLetterStore, RemoteWebhookDeadLetterStore>();
        services.AddSingleton<IWebhookCapabilitiesProvider, RemoteWebhookCapabilitiesProvider>();
        return services;
    }

    private static void RemoveWebhookPersistenceRegistrations(IServiceCollection services)
    {
        services.RemoveAll<IWebhookPersistenceProvider>();
        services.RemoveAll<IWebhookDeadLetterStore>();
        services.RemoveAll<IWebhookIngressEventStore>();
        services.RemoveAll<IWebhookEndpointChangefeed>();
    }

    private static string? ResolveSqlServerConnectionString(IConfiguration configuration)
    {
        return configuration.GetConnectionString("CroniqSqlServer")
            ?? configuration.GetConnectionString("Croniq")
            ?? configuration.GetConnectionString("DefaultConnection");
    }

    private static string? ResolvePostgresConnectionString(IConfiguration configuration)
    {
        return configuration.GetConnectionString("CroniqPostgres")
            ?? configuration.GetConnectionString("Croniq")
            ?? configuration.GetConnectionString("DefaultConnection");
    }

    public static IServiceCollection AddCroniqWebhookServices(this IServiceCollection services, IConfiguration configuration)
    {
        return AddCroniqWebhookServices(services, configuration, includePlatformServices: true);
    }

    public static IServiceCollection AddCroniqWebhookServices(
        this IServiceCollection services,
        IConfiguration configuration,
        bool includePlatformServices)
    {
        var optionsSection = configuration.GetSection("Croniq:Webhooks");
        var hostingOptions = optionsSection.Get<CroniqWebhookOptions>() ?? new CroniqWebhookOptions();
        var shouldConfigurePersistence = hostingOptions.ConfigurePersistence;

        if (includePlatformServices)
        {
            services.AddCroniqPlatformServices(configuration);
        }

        if (shouldConfigurePersistence)
        {
            services.AddCroniqWebhookPersistence(configuration);
        }

        services.Configure<CroniqWebhookOptions>(optionsSection);
        services.TryAddEnumerable(ServiceDescriptor.Singleton<IPostConfigureOptions<CroniqWebhookOptions>, CroniqWebhookOptionsPostConfigurator>());
        services.AddMemoryCache();
        services.TryAddSingleton<WebhookMetadataFactory>();
        services.TryAddSingleton<WebhookEndpointResolver>();
        services.TryAddEnumerable(ServiceDescriptor.Singleton<IWebhookEndpointChangeNotifier, WebhookEndpointCacheNotifier>());
        services.TryAddSingleton<WebhookDeadLetterRecorder>();
        services.AddHostedService<WebhookConfigurationWarningService>();
        services.AddHostedService<WebhookEndpointCacheInvalidationService>();
        if (hostingOptions.Mode == WebhookPersistenceMode.Remote)
        {
            services.AddHostedService<WebhookIngressRelayService>();
        }
        return services;
    }

    public static IServiceCollection AddCroniqWebhookRateLimiter(this IServiceCollection services)
    {
        services.AddRateLimiter(options =>
        {
            options.AddPolicy("cronq-webhooks", context =>
            {
                var hookKey = context.Request.RouteValues.TryGetValue("hookKey", out var raw)
                    ? raw?.ToString() ?? string.Empty
                    : string.Empty;
                var tenantId = context.Request.RouteValues.TryGetValue("tenantId", out var rawTenant)
                    ? rawTenant?.ToString() ?? string.Empty
                    : string.Empty;
                var environmentTag = context.Request.RouteValues.TryGetValue("environmentTag", out var rawEnv)
                    ? rawEnv?.ToString() ?? string.Empty
                    : string.Empty;

                var resolver = context.RequestServices.GetRequiredService<WebhookEndpointResolver>();
                WebhookEndpointDescriptor? descriptor = null;
                if (!string.IsNullOrWhiteSpace(tenantId) && !string.IsNullOrWhiteSpace(environmentTag) && !string.IsNullOrWhiteSpace(hookKey))
                {
                    descriptor = resolver.TryGetCached(new PartitionScope(tenantId, environmentTag), hookKey);
                }
                var limit = descriptor?.RequestsPerMinute ?? resolver.GetDefaultRequestsPerMinute();
                var partitionKey = string.IsNullOrEmpty(hookKey)
                    ? "webhooks:global"
                    : $"webhooks:{tenantId}:{environmentTag}:{hookKey}";

                if (limit <= 0)
                {
                    return RateLimitPartition.GetNoLimiter(partitionKey);
                }

                var permits = Math.Max(1, limit);
                return RateLimitPartition.GetFixedWindowLimiter(partitionKey, _ => new FixedWindowRateLimiterOptions
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

    public static WebApplication UseCroniqWebhooks(this WebApplication app, bool mapHealthEndpoints = true)
    {
        var options = app.Services.GetRequiredService<IOptions<CroniqWebhookOptions>>().Value;
        if (options.Mode == WebhookPersistenceMode.Remote)
        {
            throw new InvalidOperationException("Croniq.Webhooks ingress cannot run with Croniq:Webhooks:Mode = Remote. Use SqlServer or InMemory for ingress.");
        }

        if (options.Ingress.DispatchMode == WebhookIngressDispatchMode.StoreOnly
            && app.Services.GetService<IWebhookIngressEventStore>() is null)
        {
            throw new InvalidOperationException("Croniq:Webhooks:Ingress:DispatchMode=StoreOnly requires IWebhookIngressEventStore. Configure SqlServer persistence or register a custom store.");
        }

        if (mapHealthEndpoints)
        {
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
                    return result.IsHealthy
                        ? Results.Ok(new { status = "ok", provider = providerName, db = "reachable" })
                        : Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unhealthy", detail: result.Detail);
                }
                catch (Exception ex)
                {
                    return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unreachable", detail: ex.Message);
                }
            });
        }

        app.MapPost("/tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}", async (
            string tenantId,
            string environmentTag,
            string hookKey,
            HttpRequest request,
            IJobRegistry registry,
            IJobExecutionPipeline pipeline,
            IPolicyResolver policyResolver,
            IExecutionLogStore executionLogStore,
            IOptions<CroniqOptions> coreOptions,
            WebhookMetadataFactory metadataFactory,
            WebhookEndpointResolver endpointResolver,
            WebhookDeadLetterRecorder deadLetterRecorder,
            IOptionsMonitor<CroniqWebhookOptions> webhookOptions,
            ILogger<WebhookRequestHandlerMarker> logger,
            CancellationToken cancellationToken) =>
        {
            var scope = new PartitionScope(tenantId, environmentTag);
            var endpoint = await endpointResolver.ResolveAsync(hookKey, scope, cancellationToken).ConfigureAwait(false);
            if (endpoint is null || !endpoint.Enabled)
            {
                logger.LogWarning("webhook {HookKey} not configured", hookKey);
                return Results.NotFound(new { error = "webhook-not-found", hookKey });
            }

            if (!JobKey.TryParse(endpoint.JobKey, out var jobKey))
            {
                logger.LogWarning("webhook {HookKey} has invalid job key {JobKey}", hookKey, endpoint.JobKey);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "invalid-job-key", detail: "Configured job key is invalid.");
            }

            var remoteIp = request.HttpContext.Connection.RemoteIpAddress;
            if (!endpoint.IsIpAllowed(remoteIp))
            {
                var remoteText = remoteIp?.ToString() ?? "unknown";
                logger.LogWarning("webhook {HookKey} rejected due to remote IP {RemoteIp}", hookKey, remoteText);
                return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "ip-blocked", detail: "Remote IP address is not permitted for this webhook.");
            }

            var headers = deadLetterRecorder.CaptureHeaders(request.Headers);
            var payload = await ReadPayloadAsync(request).ConfigureAwait(false);

            var security = webhookOptions.CurrentValue.Security;
            var shouldValidateSignature = endpoint.RequireSignature || !security.AllowUnsignedHooks;

            if (shouldValidateSignature)
            {
                if (endpoint.ActiveSecrets.Count == 0)
                {
                    logger.LogWarning("webhook {HookKey} requires signature but no active secrets are available", hookKey);
                    return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "missing-secret", detail: "Webhook requires a secret for signature validation.");
                }

                var provided = request.Headers["X-Croniq-Signature"].FirstOrDefault();
                if (string.IsNullOrWhiteSpace(provided))
                {
                    await deadLetterRecorder.TryRecordAsync(jobKey, scope, endpoint, payload, headers, metadata: null, failureReason: "signature-missing", statusCode: StatusCodes.Status401Unauthorized, errorDetails: "missing signature header", cancellationToken: cancellationToken).ConfigureAwait(false);
                    return Results.StatusCode(StatusCodes.Status401Unauthorized);
                }

                var accepted = false;
                foreach (var secret in endpoint.ActiveSecrets)
                {
                    var expected = ComputeSignature(secret, payload);
                    if (FixedTimeEquals(expected, provided))
                    {
                        accepted = true;
                        break;
                    }
                }

                if (!accepted)
                {
                    logger.LogWarning("signature mismatch for webhook {HookKey}", hookKey);
                    await deadLetterRecorder.TryRecordAsync(jobKey, scope, endpoint, payload, headers, metadata: null, failureReason: "signature-invalid", statusCode: StatusCodes.Status401Unauthorized, errorDetails: "signature mismatch", cancellationToken: cancellationToken).ConfigureAwait(false);
                    return Results.StatusCode(StatusCodes.Status401Unauthorized);
                }
            }
            else
            {
                if (UnsignedWarningCache.TryAdd(endpoint.HookKey, 0))
                {
                    logger.LogWarning("webhook {HookKey} accepts unsigned payloads because AllowUnsignedHooks=true", endpoint.HookKey);
                }
            }

            var metadata = metadataFactory.Create(endpoint, payload);
            var ingressOptions = webhookOptions.CurrentValue.Ingress;
            if (ingressOptions.DispatchMode == WebhookIngressDispatchMode.StoreOnly)
            {
                var ingressStore = request.HttpContext.RequestServices.GetService<IWebhookIngressEventStore>();
                if (ingressStore is null)
                {
                    logger.LogError("Webhook ingress store is not configured while DispatchMode=StoreOnly.");
                    return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "ingress-store-missing", detail: "Webhook ingress store is not configured.");
                }

                using var storeActivity = ActivitySource.StartActivity("Croniq.Webhooks.Ingress.Store", ActivityKind.Server);
                storeActivity?.SetTag("croniq.webhook.key", endpoint.HookKey);
                storeActivity?.SetTag("croniq.job.key", jobKey.Value);
                storeActivity?.SetTag("croniq.tenant_id", IdentifierHashing.HashTenantId(scope.TenantId));
                storeActivity?.SetTag("croniq.environment", scope.EnvironmentTag);

                var eventId = Guid.NewGuid().ToString("N");
                try
                {
                    await ingressStore.EnqueueAsync(
                        new WebhookIngressEventCreate(
                            eventId,
                            endpoint.HookKey,
                            endpoint.JobKey,
                            scope.TenantId,
                            scope.EnvironmentTag,
                            payload,
                            headers,
                            metadata,
                            DateTimeOffset.UtcNow),
                        cancellationToken).ConfigureAwait(false);

                    storeActivity?.SetStatus(ActivityStatusCode.Ok);
                    return Results.Accepted(value: new { status = "stored", eventId, hook = endpoint.HookKey, job = endpoint.JobKey });
                }
                catch (Exception ex)
                {
                    storeActivity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                    logger.LogError(ex, "error storing webhook {HookKey} ingress event", endpoint.HookKey);
                    await deadLetterRecorder.TryRecordAsync(jobKey, scope, endpoint, payload, headers, metadata, failureReason: "ingress-store-failed", statusCode: StatusCodes.Status500InternalServerError, errorDetails: ex.Message, cancellationToken: cancellationToken).ConfigureAwait(false);
                    return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "ingress-store-failed", detail: ex.Message);
                }
            }

            if (!registry.TryGet(jobKey, out var descriptor))
            {
                logger.LogWarning("job {JobKey} not registered for webhook {HookKey}", endpoint.JobKey, hookKey);
                await deadLetterRecorder.TryRecordAsync(jobKey, scope, endpoint, payload, headers, metadata: null, failureReason: "job-not-registered", statusCode: StatusCodes.Status404NotFound, errorDetails: "job not registered", cancellationToken: cancellationToken).ConfigureAwait(false);
                return Results.NotFound(new { error = "job-not-registered", endpoint.JobKey });
            }

            var executionOptions = policyResolver.ResolveExecution(jobKey, scope);
            var executionId = Guid.NewGuid().ToString("N");
            var execRequest = new JobExecutionRequest(executionId, jobKey, scope, descriptor, executionOptions, metadata, ActivitySource);
            var startedAtUtc = DateTimeOffset.UtcNow;

            using var activity = ActivitySource.StartActivity("Croniq.Webhooks.Trigger", ActivityKind.Server);
            activity?.SetTag("croniq.webhook.key", endpoint.HookKey);
            activity?.SetTag("croniq.job.key", jobKey.Value);
            activity?.SetTag("croniq.tenant_id", IdentifierHashing.HashTenantId(scope.TenantId));
            activity?.SetTag("croniq.environment", scope.EnvironmentTag);
            await TryStoreExecutionStartedAsync(
                executionLogStore,
                logger,
                new ExecutionRecord(
                    executionId,
                    ExecutionKind.Job,
                    WorkflowId: null,
                    jobKey.Value,
                    scope.TenantId,
                    scope.EnvironmentTag,
                    TriggerId: null,
                    FireAtUtc: startedAtUtc,
                    StartedAtUtc: startedAtUtc,
                    coreOptions.Value.InstanceId,
                    activity?.TraceId.ToString(),
                    activity?.SpanId.ToString(),
                    TryGetCorrelationId(activity, metadata)),
                cancellationToken).ConfigureAwait(false);

            var stopwatch = Stopwatch.StartNew();
            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                stopwatch.Stop();
                activity?.SetStatus(ActivityStatusCode.Ok);
                await TryStoreExecutionCompletedAsync(
                    executionLogStore,
                    logger,
                    executionId,
                    ExecutionStatus.Succeeded,
                    stopwatch.Elapsed.TotalMilliseconds,
                    error: null,
                    cancellationToken).ConfigureAwait(false);
                return Results.Accepted(value: new { status = "triggered", hook = endpoint.HookKey, job = endpoint.JobKey });
            }
            catch (Exception ex)
            {
                stopwatch.Stop();
                var canceled = IsCancellation(ex, cancellationToken);
                await TryStoreExecutionCompletedAsync(
                    executionLogStore,
                    logger,
                    executionId,
                    canceled ? ExecutionStatus.Canceled : ExecutionStatus.Failed,
                    stopwatch.Elapsed.TotalMilliseconds,
                    canceled ? null : ex,
                    cancellationToken).ConfigureAwait(false);
                activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                logger.LogError(ex, "error executing job {JobKey} for webhook {HookKey}", endpoint.JobKey, endpoint.HookKey);
                await deadLetterRecorder.TryRecordAsync(jobKey, scope, endpoint, payload, headers, metadata, failureReason: "execution-error", statusCode: StatusCodes.Status500InternalServerError, errorDetails: ex.Message, cancellationToken: cancellationToken).ConfigureAwait(false);
                throw;
            }
        }).RequireRateLimiting("cronq-webhooks");

        return app;
    }

    private static async Task<string> ReadPayloadAsync(HttpRequest request)
    {
        if (request.Body.CanSeek)
        {
            request.Body.Position = 0;
        }

        using var reader = new StreamReader(request.Body, Encoding.UTF8, leaveOpen: true);
        var payload = await reader.ReadToEndAsync().ConfigureAwait(false);

        if (request.Body.CanSeek)
        {
            request.Body.Position = 0;
        }

        return payload;
    }

    private static string ComputeSignature(string secret, string payload)
    {
        var keyBytes = Encoding.UTF8.GetBytes(secret);
        var payloadBytes = Encoding.UTF8.GetBytes(payload ?? string.Empty);
        var hash = HMACSHA256.HashData(keyBytes, payloadBytes);
        return $"sha256={Convert.ToHexString(hash).ToLowerInvariant()}";
    }

    private static bool FixedTimeEquals(string expected, string provided)
    {
        var expectedBytes = Encoding.UTF8.GetBytes(expected);
        var providedBytes = Encoding.UTF8.GetBytes(provided);
        if (expectedBytes.Length != providedBytes.Length)
        {
            return false;
        }

        return CryptographicOperations.FixedTimeEquals(expectedBytes, providedBytes);
    }

    private static bool IsCancellation(Exception exception, CancellationToken cancellationToken)
        => cancellationToken.IsCancellationRequested && exception is OperationCanceledException;

    private static string? TryGetCorrelationId(Activity? activity, IReadOnlyDictionary<string, string>? metadata)
    {
        if (activity?.GetBaggageItem("croniq.correlation_id") is { Length: > 0 } baggageCorrelation)
        {
            return baggageCorrelation;
        }

        if (activity?.GetTagItem("croniq.correlation_id") is string tagCorrelation && !string.IsNullOrWhiteSpace(tagCorrelation))
        {
            return tagCorrelation;
        }

        if (metadata is not null && metadata.TryGetValue("correlation_id", out var value) && !string.IsNullOrWhiteSpace(value))
        {
            return value;
        }

        return null;
    }

    private static async Task TryStoreExecutionStartedAsync(
        IExecutionLogStore executionLogStore,
        ILogger logger,
        ExecutionRecord record,
        CancellationToken cancellationToken)
    {
        try
        {
            await executionLogStore.OnExecutionStartedAsync(record, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to persist execution start for {ExecutionId}", record.ExecutionId);
        }
    }

    private static async Task TryStoreExecutionCompletedAsync(
        IExecutionLogStore executionLogStore,
        ILogger logger,
        string executionId,
        ExecutionStatus status,
        double? durationMs,
        Exception? error,
        CancellationToken cancellationToken)
    {
        try
        {
            var completion = new ExecutionCompletion(
                executionId,
                DateTimeOffset.UtcNow,
                status,
                durationMs,
                error?.GetType().FullName ?? error?.GetType().Name,
                error?.Message);

            await executionLogStore.OnExecutionCompletedAsync(completion, cancellationToken).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "Failed to persist execution completion for {ExecutionId}", executionId);
        }
    }

    private sealed class WebhookMetadataFactory
    {
        public IReadOnlyDictionary<string, string> Create(WebhookEndpointDescriptor endpoint, string payload)
        {
            var metadata = endpoint.Metadata is null
                ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                : new Dictionary<string, string>(endpoint.Metadata, StringComparer.OrdinalIgnoreCase);

            metadata["webhook:hook"] = endpoint.HookKey;
            if (!string.IsNullOrWhiteSpace(payload))
            {
                metadata["webhook:payload"] = payload;
                TryAddJsonHints(metadata, payload);
            }

            return metadata;
        }

        private static void TryAddJsonHints(IDictionary<string, string> metadata, string payload)
        {
            try
            {
                using var document = JsonDocument.Parse(payload);
                if (document.RootElement.ValueKind != JsonValueKind.Object)
                {
                    return;
                }

                foreach (var property in document.RootElement.EnumerateObject())
                {
                    var key = $"payload:{property.Name}";
                    switch (property.Value.ValueKind)
                    {
                        case JsonValueKind.String:
                            metadata[key] = property.Value.GetString() ?? string.Empty;
                            break;
                        case JsonValueKind.Number when property.Value.TryGetDecimal(out var number):
                            metadata[key] = number.ToString(CultureInfo.InvariantCulture);
                            break;
                        case JsonValueKind.True:
                        case JsonValueKind.False:
                            metadata[key] = property.Value.GetBoolean().ToString();
                            break;
                    }
                }
            }
            catch (JsonException)
            {
                // ignore malformed payloads
            }
        }
    }

    private sealed class WebhookRequestHandlerMarker
    {
    }

    private sealed class WebhookDeadLetterRecorder
    {
        private const int HeaderSnapshotLimit = 32;
        private static readonly IReadOnlyDictionary<string, string> EmptyHeaders = new Dictionary<string, string>();
        private readonly IWebhookDeadLetterStore? _store;
        private readonly IOptionsMonitor<CroniqWebhookOptions> _options;
        private readonly ILogger<WebhookDeadLetterRecorder> _logger;

        public WebhookDeadLetterRecorder(
            IServiceProvider services,
            IOptionsMonitor<CroniqWebhookOptions> options,
            ILogger<WebhookDeadLetterRecorder> logger)
        {
            _store = services.GetService<IWebhookDeadLetterStore>();
            _options = options;
            _logger = logger;
        }

        public IReadOnlyDictionary<string, string> CaptureHeaders(IHeaderDictionary headers)
        {
            if (headers is null || headers.Count == 0)
            {
                return EmptyHeaders;
            }

            var snapshot = new Dictionary<string, string>(Math.Min(HeaderSnapshotLimit, headers.Count), StringComparer.OrdinalIgnoreCase);
            var count = 0;
            foreach (var header in headers)
            {
                if (count++ >= HeaderSnapshotLimit)
                {
                    break;
                }

                snapshot[header.Key] = string.Join(',', header.Value.ToArray());
            }

            return snapshot;
        }

        public async Task TryRecordAsync(
            JobKey jobKey,
            PartitionScope scope,
            WebhookEndpointDescriptor endpoint,
            string payload,
            IReadOnlyDictionary<string, string> headers,
            IReadOnlyDictionary<string, string>? metadata,
            string failureReason,
            int statusCode,
            string? errorDetails,
            CancellationToken cancellationToken)
        {
            if (_store is null)
            {
                return;
            }

            var options = _options.CurrentValue;
            if (!options.DeadLetter.Enabled)
            {
                return;
            }

            try
            {
                DateTimeOffset? expiry = options.DeadLetter.RetentionDays > 0
                    ? DateTimeOffset.UtcNow.AddDays(options.DeadLetter.RetentionDays)
                    : (DateTimeOffset?)null;

                var create = new WebhookDeadLetterCreate(
                    endpoint.HookKey,
                    jobKey.Value,
                    scope.TenantId,
                    scope.EnvironmentTag,
                    payload ?? string.Empty,
                    headers,
                    metadata,
                    failureReason,
                    statusCode,
                    errorDetails,
                    expiry);

                await _store.CreateAsync(create, cancellationToken).ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                _logger.LogError(ex, "failed to record webhook dead letter for {HookKey}", endpoint.HookKey);
            }
        }
    }

    private sealed class WebhookEndpointCacheInvalidationService : BackgroundService
    {
        private readonly WebhookEndpointResolver _resolver;
        private readonly IOptionsMonitor<CroniqWebhookOptions> _options;
        private readonly ILogger<WebhookEndpointCacheInvalidationService> _logger;
        private readonly IWebhookEndpointChangefeed? _changefeed;
        private long _cursor;

        public WebhookEndpointCacheInvalidationService(
            WebhookEndpointResolver resolver,
            IOptionsMonitor<CroniqWebhookOptions> options,
            ILogger<WebhookEndpointCacheInvalidationService> logger,
            IWebhookEndpointChangefeed? changefeed = null)
        {
            _resolver = resolver;
            _options = options;
            _logger = logger;
            _changefeed = changefeed;
        }

        protected override async Task ExecuteAsync(CancellationToken stoppingToken)
        {
            if (_changefeed is null)
            {
                _logger.LogDebug("Webhook changefeed not configured; cache invalidation disabled.");
                return;
            }

            _logger.LogDebug("Webhook changefeed cache invalidation started.");

            while (!stoppingToken.IsCancellationRequested)
            {
                try
                {
                    var cacheOptions = _options.CurrentValue.Cache;
                    if (!cacheOptions.ChangefeedEnabled)
                    {
                        await DelayAsync(cacheOptions.PollingIntervalSeconds, stoppingToken).ConfigureAwait(false);
                        continue;
                    }

                    var batchSize = Math.Max(1, cacheOptions.BatchSize);
                    var batch = await _changefeed.FetchAsync(_cursor, batchSize, stoppingToken).ConfigureAwait(false);
                    if (batch.Count == 0)
                    {
                        await DelayAsync(cacheOptions.PollingIntervalSeconds, stoppingToken).ConfigureAwait(false);
                        continue;
                    }

                    foreach (var evt in batch)
                    {
                        _resolver.Invalidate(new PartitionScope(evt.TenantId, evt.EnvironmentTag), evt.HookKey);
                        _cursor = Math.Max(_cursor, evt.Id);
                        _logger.LogDebug("Invalidated webhook cache for {HookKey} due to {EventType}", evt.HookKey, evt.EventType);
                    }
                }
                catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
                {
                    break;
                }
                catch (Exception ex)
                {
                    _logger.LogWarning(ex, "Webhook cache invalidation loop failed; backing off.");
                    await DelayAsync(5, stoppingToken).ConfigureAwait(false);
                }
            }

            _logger.LogDebug("Webhook changefeed cache invalidation stopped.");
        }

        private static Task DelayAsync(int seconds, CancellationToken cancellationToken)
        {
            var delay = TimeSpan.FromSeconds(Math.Max(1, seconds));
            return Task.Delay(delay, cancellationToken);
        }
    }

    private sealed class WebhookConfigurationWarningService : IHostedService
    {
        private readonly IOptionsMonitor<CroniqWebhookOptions> _options;
        private readonly ILogger<WebhookConfigurationWarningService> _logger;

        public WebhookConfigurationWarningService(
            IOptionsMonitor<CroniqWebhookOptions> options,
            ILogger<WebhookConfigurationWarningService> logger)
        {
            _options = options;
            _logger = logger;
        }

        public Task StartAsync(CancellationToken cancellationToken)
        {
            var options = _options.CurrentValue;
            if ((options.Mode == WebhookPersistenceMode.SqlServer || options.Mode == WebhookPersistenceMode.Postgres)
                && options.Endpoints.Count > 0)
            {
                _logger.LogWarning(
                    "Croniq:Webhooks:Endpoints is configured while Mode={Mode}; persisted webhooks take precedence and config-defined endpoints are used only when no persisted hook exists for the same key. Configured endpoints: {Count}",
                    options.Mode,
                    options.Endpoints.Count);
            }

            return Task.CompletedTask;
        }

        public Task StopAsync(CancellationToken cancellationToken) => Task.CompletedTask;
    }

    private sealed class WebhookEndpointResolver
    {
        private readonly IWebhookPersistenceProvider? _store;
        private readonly IOptionsMonitor<CroniqWebhookOptions> _options;
        private readonly IMemoryCache _cache;

        public WebhookEndpointResolver(
            IWebhookPersistenceProvider? store,
            IOptionsMonitor<CroniqWebhookOptions> options,
            IMemoryCache cache)
        {
            _store = store;
            _options = options;
            _cache = cache;
        }

        public int GetDefaultRequestsPerMinute()
        {
            var configured = _options.CurrentValue.RequestsPerMinute;
            return configured <= 0 ? 1 : configured;
        }

        public WebhookEndpointDescriptor? TryGetCached(PartitionScope scope, string hookKey)
        {
            if (string.IsNullOrWhiteSpace(hookKey))
            {
                return null;
            }

            if (_cache.TryGetValue(BuildEndpointCacheKey(scope, hookKey), out var cached) && cached is WebhookEndpointDescriptor descriptor)
            {
                return descriptor;
            }

            return null;
        }

        public async Task<WebhookEndpointDescriptor?> ResolveAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
        {
            if (string.IsNullOrWhiteSpace(hookKey))
            {
                return null;
            }

            if (_cache.TryGetValue(BuildEndpointCacheKey(scope, hookKey), out var cached) && cached is WebhookEndpointDescriptor descriptor)
            {
                return descriptor;
            }

            if (_store is not null)
            {
                var persisted = await _store.FindByHookKeyAsync(hookKey, scope, cancellationToken).ConfigureAwait(false);
                if (persisted is not null)
                {
                    var secrets = await ResolveActiveSecretsAsync(persisted, new PartitionScope(persisted.TenantId, persisted.EnvironmentTag), cancellationToken).ConfigureAwait(false);
                    descriptor = WebhookEndpointDescriptor.FromDefinition(persisted, secrets);
                    _cache.Set(BuildEndpointCacheKey(scope, hookKey), descriptor, TimeSpan.FromMinutes(1));
                    return descriptor;
                }
            }

            var options = _options.CurrentValue;
            var config = options.Endpoints
                .Where(e => e.Enabled && string.Equals(e.HookKey, hookKey, StringComparison.OrdinalIgnoreCase))
                .FirstOrDefault(e => JobKey.TryParse(e.JobKey, out _));
            if (config is not null)
            {
                descriptor = WebhookEndpointDescriptor.FromOptions(config, options.RequestsPerMinute, options.Security);
                _cache.Set(BuildEndpointCacheKey(scope, hookKey), descriptor, TimeSpan.FromSeconds(30));
                return descriptor;
            }

            return null;
        }

        public void Invalidate(PartitionScope scope, string hookKey)
        {
            if (string.IsNullOrWhiteSpace(hookKey))
            {
                return;
            }

            _cache.Remove(BuildEndpointCacheKey(scope, hookKey));
        }

        private async Task<IReadOnlyList<string>> ResolveActiveSecretsAsync(WebhookEndpointDefinition definition, PartitionScope scope, CancellationToken cancellationToken)
        {
            if (_store is null)
            {
                return NormalizeSecrets(null, definition.Secret);
            }

            var materials = await _store.GetActiveSecretsAsync(definition.HookKey, scope, cancellationToken).ConfigureAwait(false);
            if (materials.Count == 0)
            {
                return NormalizeSecrets(null, definition.Secret);
            }

            var activeSecrets = materials
                .Select(material => material.Secret)
                .Where(secret => !string.IsNullOrWhiteSpace(secret));

            return NormalizeSecrets(activeSecrets, definition.Secret);
        }

        private static IReadOnlyList<string> NormalizeSecrets(IEnumerable<string>? secrets, string? fallback)
        {
            var ordered = new List<string>();
            var unique = new HashSet<string>(StringComparer.Ordinal);

            void TryAdd(string? candidate)
            {
                if (string.IsNullOrWhiteSpace(candidate))
                {
                    return;
                }

                if (unique.Add(candidate))
                {
                    ordered.Add(candidate);
                }
            }

            if (secrets is not null)
            {
                foreach (var secret in secrets)
                {
                    TryAdd(secret);
                }
            }

            if (ordered.Count == 0)
            {
                TryAdd(fallback);
            }

            return ordered.Count == 0 ? Array.Empty<string>() : ordered;
        }
    }

    private sealed class WebhookEndpointCacheNotifier : IWebhookEndpointChangeNotifier
    {
        private readonly IMemoryCache _cache;

        public WebhookEndpointCacheNotifier(IMemoryCache cache)
        {
            _cache = cache;
        }

        public void NotifyChanged(string hookKey, PartitionScope scope)
        {
            if (string.IsNullOrWhiteSpace(hookKey))
            {
                return;
            }

            _cache.Remove(BuildEndpointCacheKey(scope, hookKey));
        }
    }

    private sealed class CroniqWebhookOptionsPostConfigurator : IPostConfigureOptions<CroniqWebhookOptions>
    {
        private readonly IHostEnvironment _environment;

        public CroniqWebhookOptionsPostConfigurator(IHostEnvironment environment)
        {
            _environment = environment ?? throw new ArgumentNullException(nameof(environment));
        }

        public void PostConfigure(string? name, CroniqWebhookOptions options)
        {
            if (options is null)
            {
                return;
            }

            if (!options.Security.AllowUnsignedHooks)
            {
                var unsignedHooks = options.Endpoints
                    .Where(endpoint => endpoint.Enabled && !endpoint.RequireSignature)
                    .Select(endpoint => endpoint.HookKey)
                    .Where(hookKey => !string.IsNullOrWhiteSpace(hookKey))
                    .ToArray();

                if (unsignedHooks.Length > 0)
                {
                    var joined = string.Join(", ", unsignedHooks);
                    throw new InvalidOperationException($"Unsigned webhooks ({joined}) are configured but Croniq:Webhooks:Security:AllowUnsignedHooks is disabled.");
                }
            }

            if (options.Mode == WebhookPersistenceMode.Remote
                && (options.Remote?.AllowInvalidServerCertificate ?? false)
                && !_environment.IsDevelopment())
            {
                throw new InvalidOperationException("Croniq:Webhooks:Remote:AllowInvalidServerCertificate is only supported in Development.");
            }
        }
    }

    private sealed class WebhookEndpointDescriptor
    {
        private WebhookEndpointDescriptor(
            string hookKey,
            string jobKey,
            bool requireSignature,
            bool enabled,
            int requestsPerMinute,
            IReadOnlyDictionary<string, string>? metadata,
            IReadOnlyList<string> activeSecrets,
            IReadOnlyList<IpNetwork> allowedNetworks)
        {
            HookKey = hookKey;
            JobKey = jobKey;
            RequireSignature = requireSignature;
            Enabled = enabled;
            RequestsPerMinute = Math.Max(1, requestsPerMinute);
            Metadata = metadata;
            ActiveSecrets = activeSecrets;
            AllowedNetworks = allowedNetworks;
        }

        public string HookKey { get; }
        public string JobKey { get; }
        public bool RequireSignature { get; }
        public bool Enabled { get; }
        public int RequestsPerMinute { get; }
        public IReadOnlyDictionary<string, string>? Metadata { get; }
        public IReadOnlyList<string> ActiveSecrets { get; }
        public IReadOnlyList<IpNetwork> AllowedNetworks { get; }

        public bool IsIpAllowed(IPAddress? address)
        {
            if (AllowedNetworks.Count == 0)
            {
                return true;
            }

            if (address is null)
            {
                return false;
            }

            foreach (var network in AllowedNetworks)
            {
                if (network.Contains(address))
                {
                    return true;
                }
            }

            return false;
        }

        public static WebhookEndpointDescriptor FromDefinition(WebhookEndpointDefinition definition, IReadOnlyList<string> activeSecrets)
        {
            var metadata = definition.Metadata is null
                ? null
                : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

            var networks = BuildNetworks(definition.IpRules);
            return new WebhookEndpointDescriptor(
                definition.HookKey,
                definition.JobKey,
                definition.RequireSignature,
                definition.Enabled,
                definition.RequestsPerMinute,
                metadata,
                activeSecrets,
                networks);
        }

        public static WebhookEndpointDescriptor FromOptions(WebhookEndpointOptions options, int defaultLimit, WebhookSecurityOptions security)
        {
            if (string.IsNullOrWhiteSpace(options.Secret))
            {
                throw new InvalidOperationException($"Webhook {options.HookKey} requires a secret to process requests.");
            }

            if (!options.RequireSignature && !(security?.AllowUnsignedHooks ?? false))
            {
                throw new InvalidOperationException($"Webhook {options.HookKey} disables signature validation, but unsigned hooks are not permitted.");
            }

            var limit = options.RequestsPerMinute ?? defaultLimit;
            var metadata = options.Metadata is null
                ? null
                : new Dictionary<string, string>(options.Metadata, StringComparer.OrdinalIgnoreCase);

            return new WebhookEndpointDescriptor(
                options.HookKey,
                options.JobKey,
                options.RequireSignature,
                options.Enabled,
                limit,
                metadata,
                new List<string> { options.Secret },
                Array.Empty<IpNetwork>());
        }

        private static IReadOnlyList<IpNetwork> BuildNetworks(IReadOnlyCollection<WebhookIpRuleDefinition> ipRules)
        {
            if (ipRules is null || ipRules.Count == 0)
            {
                return Array.Empty<IpNetwork>();
            }

            var networks = new List<IpNetwork>(ipRules.Count);
            foreach (var rule in ipRules)
            {
                if (IpNetwork.TryParse(rule.Cidr, out var network, out _)
                    && network is not null)
                {
                    networks.Add(network);
                }
            }

            return networks.Count == 0 ? Array.Empty<IpNetwork>() : networks;
        }
    }
}
