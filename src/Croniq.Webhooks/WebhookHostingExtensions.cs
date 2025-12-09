using System.Diagnostics;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Hosting;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks.Options;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.Caching.Memory;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using System.Threading.RateLimiting;

namespace Croniq.Webhooks;

public static class WebhookHostingExtensions
{
    private static readonly ActivitySource ActivitySource = new("Croniq.Webhooks.Ingress");

    public static IServiceCollection AddCroniqWebhookServices(this IServiceCollection services, IConfiguration configuration)
    {
        services.AddCroniqPlatformServices(configuration);
        services.Configure<CroniqWebhookOptions>(configuration.GetSection("Croniq:Webhooks"));
        services.AddMemoryCache();
        services.TryAddSingleton<WebhookMetadataFactory>();
        services.TryAddSingleton<WebhookEndpointResolver>();
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
                var resolver = context.RequestServices.GetRequiredService<WebhookEndpointResolver>();
                var descriptor = resolver.TryGetCached(hookKey);
                var limit = descriptor?.RequestsPerMinute ?? resolver.GetDefaultRequestsPerMinute();
                var partitionKey = string.IsNullOrEmpty(hookKey) ? "webhooks:global" : $"webhooks:{hookKey}";

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

    public static WebApplication UseCroniqWebhooks(this WebApplication app)
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

        app.MapPost("/webhooks/{hookKey}", async (
            string hookKey,
            HttpRequest request,
            IJobRegistry registry,
            IJobExecutionPipeline pipeline,
            IPolicyResolver policyResolver,
            WebhookMetadataFactory metadataFactory,
            WebhookEndpointResolver endpointResolver,
            ILogger<WebhookRequestHandlerMarker> logger,
            CancellationToken cancellationToken) =>
        {
            var endpoint = await endpointResolver.ResolveAsync(hookKey, cancellationToken).ConfigureAwait(false);
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

            if (!registry.TryGet(jobKey, out var descriptor))
            {
                logger.LogWarning("job {JobKey} not registered for webhook {HookKey}", endpoint.JobKey, hookKey);
                return Results.NotFound(new { error = "job-not-registered", endpoint.JobKey });
            }

            var payload = await ReadPayloadAsync(request).ConfigureAwait(false);

            if (endpoint.RequireSignature)
            {
                if (string.IsNullOrWhiteSpace(endpoint.Secret))
                {
                    logger.LogWarning("webhook {HookKey} requires signature but no secret configured", hookKey);
                    return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "missing-secret", detail: "Webhook requires a secret for signature validation.");
                }

                var provided = request.Headers["X-Croniq-Signature"].FirstOrDefault();
                if (string.IsNullOrWhiteSpace(provided))
                {
                    return Results.StatusCode(StatusCodes.Status401Unauthorized);
                }

                var expected = ComputeSignature(endpoint.Secret, payload);
                if (!FixedTimeEquals(expected, provided))
                {
                    logger.LogWarning("signature mismatch for webhook {HookKey}", hookKey);
                    return Results.StatusCode(StatusCodes.Status401Unauthorized);
                }
            }

            var metadata = metadataFactory.Create(endpoint, payload);
            var executionOptions = policyResolver.ResolveExecution(jobKey);
            var execRequest = new JobExecutionRequest(jobKey, descriptor, executionOptions, metadata, ActivitySource);

            using var activity = ActivitySource.StartActivity("Croniq.Webhooks.Trigger", ActivityKind.Server);
            activity?.SetTag("croniq.webhook.key", endpoint.HookKey);
            activity?.SetTag("croniq.job.key", jobKey.Value);
            activity?.SetTag("croniq.tenant_id", jobKey.TenantId);
            activity?.SetTag("croniq.environment", jobKey.EnvironmentTag);

            try
            {
                await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
                activity?.SetStatus(ActivityStatusCode.Ok);
                return Results.Accepted(value: new { status = "triggered", hook = endpoint.HookKey, job = endpoint.JobKey });
            }
            catch (Exception ex)
            {
                activity?.SetStatus(ActivityStatusCode.Error, ex.Message);
                logger.LogError(ex, "error executing job {JobKey} for webhook {HookKey}", endpoint.JobKey, endpoint.HookKey);
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

        public WebhookEndpointDescriptor? TryGetCached(string hookKey)
        {
            if (string.IsNullOrWhiteSpace(hookKey))
            {
                return null;
            }

            if (_cache.TryGetValue(GetCacheKey(hookKey), out var cached) && cached is WebhookEndpointDescriptor descriptor)
            {
                return descriptor;
            }

            return null;
        }

        public async Task<WebhookEndpointDescriptor?> ResolveAsync(string hookKey, CancellationToken cancellationToken)
        {
            if (string.IsNullOrWhiteSpace(hookKey))
            {
                return null;
            }

            if (_cache.TryGetValue(GetCacheKey(hookKey), out var cached) && cached is WebhookEndpointDescriptor descriptor)
            {
                return descriptor;
            }

            if (_store is not null)
            {
                var persisted = await _store.FindByHookKeyAsync(hookKey, cancellationToken).ConfigureAwait(false);
                if (persisted is not null)
                {
                    descriptor = WebhookEndpointDescriptor.FromDefinition(persisted);
                    _cache.Set(GetCacheKey(hookKey), descriptor, TimeSpan.FromMinutes(1));
                    return descriptor;
                }
            }

            var options = _options.CurrentValue;
            var config = options.Endpoints.FirstOrDefault(e => e.Enabled && string.Equals(e.HookKey, hookKey, StringComparison.OrdinalIgnoreCase));
            if (config is not null)
            {
                descriptor = WebhookEndpointDescriptor.FromOptions(config, options.RequestsPerMinute);
                _cache.Set(GetCacheKey(hookKey), descriptor, TimeSpan.FromSeconds(30));
                return descriptor;
            }

            return null;
        }

        private static string GetCacheKey(string hookKey) => $"webhook:endpoint:{hookKey.ToLowerInvariant()}";
    }

    private sealed class WebhookEndpointDescriptor
    {
        private WebhookEndpointDescriptor(
            string hookKey,
            string jobKey,
            string secret,
            bool requireSignature,
            bool enabled,
            int requestsPerMinute,
            IReadOnlyDictionary<string, string>? metadata)
        {
            HookKey = hookKey;
            JobKey = jobKey;
            Secret = secret;
            RequireSignature = requireSignature;
            Enabled = enabled;
            RequestsPerMinute = Math.Max(1, requestsPerMinute);
            Metadata = metadata;
        }

        public string HookKey { get; }
        public string JobKey { get; }
        public string Secret { get; }
        public bool RequireSignature { get; }
        public bool Enabled { get; }
        public int RequestsPerMinute { get; }
        public IReadOnlyDictionary<string, string>? Metadata { get; }

        public static WebhookEndpointDescriptor FromDefinition(WebhookEndpointDefinition definition)
        {
            var metadata = definition.Metadata is null
                ? null
                : new Dictionary<string, string>(definition.Metadata, StringComparer.OrdinalIgnoreCase);

            return new WebhookEndpointDescriptor(
                definition.HookKey,
                definition.JobKey,
                definition.Secret,
                definition.RequireSignature,
                definition.Enabled,
                definition.RequestsPerMinute,
                metadata);
        }

        public static WebhookEndpointDescriptor FromOptions(WebhookEndpointOptions options, int defaultLimit)
        {
            if (string.IsNullOrWhiteSpace(options.Secret))
            {
                throw new InvalidOperationException($"Webhook {options.HookKey} requires a secret to process requests.");
            }

            var limit = options.RequestsPerMinute ?? defaultLimit;
            var metadata = options.Metadata is null
                ? null
                : new Dictionary<string, string>(options.Metadata, StringComparer.OrdinalIgnoreCase);

            return new WebhookEndpointDescriptor(
                options.HookKey,
                options.JobKey,
                options.Secret,
                options.RequireSignature,
                options.Enabled,
                limit,
                metadata);
        }
    }
}
