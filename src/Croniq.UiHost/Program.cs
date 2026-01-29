using System.Text.Json;
using System.Text.Json.Serialization;
using Croniq.Core;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

var browserWebRootPath = ResolveBrowserWebRootPath(args);

var builderOptions = new WebApplicationOptions
{
    Args = args,
    WebRootPath = browserWebRootPath
};

var builder = WebApplication.CreateBuilder(builderOptions);

builder.Services.AddHealthChecks();

var observability = builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Ui");

observability.WithTracing(tracing =>
{
    tracing.AddAspNetCoreInstrumentation(options => options.RecordException = true);
});

observability.WithMetrics(metrics =>
{
    metrics.AddAspNetCoreInstrumentation();
});

var app = builder.Build();

app.MapHealthChecks("/health");

app.Map("/assets/croniq-config.json", configApp =>
{
    configApp.Run(async context =>
    {
        var config = LoadRuntimeConfig(context.RequestServices.GetRequiredService<IWebHostEnvironment>(), app.Logger);
        context.Response.Headers.CacheControl = "no-store";
        await context.Response.WriteAsJsonAsync(config, JsonOptions.Output);
    });
});

app.UseDefaultFiles();
app.UseStaticFiles();
app.MapFallbackToFile("index.html");

app.Run();

static RuntimeConfig LoadRuntimeConfig(IWebHostEnvironment env, ILogger logger)
{
    var webRootPath = env.WebRootPath ?? Path.Combine(env.ContentRootPath, "wwwroot");
    var configPath = Path.Combine(webRootPath, "assets", "croniq-config.json");
    var fromFile = LoadRuntimeConfigFromFile(configPath, logger);

    var apiBaseUrl = ResolveApiBaseUrl();
    var swaggerUiUrl = ResolveSwaggerUiUrl();
    var defaultTenantId = ResolveDefaultTenantId();
    var streamMode = ResolveWebhooksActivityStreamMode();
    var grpcBaseUrl = ResolveWebhooksActivityGrpcBaseUrl();
    var sseBaseUrl = ResolveWebhooksActivitySseBaseUrl();
    var runnerStreamMode = ResolveRunnersPresenceStreamMode();
    var runnerGrpcBaseUrl = ResolveRunnersPresenceGrpcBaseUrl();
    var runnerSseBaseUrl = ResolveRunnersPresenceSseBaseUrl();
    var webhooks = MergeWebhooksConfig(fromFile.Webhooks, streamMode, grpcBaseUrl, sseBaseUrl);
    var runners = MergeRunnersConfig(fromFile.Runners, runnerStreamMode, runnerGrpcBaseUrl, runnerSseBaseUrl);

    return fromFile with
    {
        ApiBaseUrl = apiBaseUrl ?? fromFile.ApiBaseUrl,
        SwaggerUiUrl = swaggerUiUrl ?? fromFile.SwaggerUiUrl,
        DefaultTenantId = defaultTenantId ?? fromFile.DefaultTenantId,
        Webhooks = webhooks,
        Runners = runners
    };
}

static RuntimeConfig LoadRuntimeConfigFromFile(string path, ILogger logger)
{
    if (!File.Exists(path))
    {
        return new RuntimeConfig();
    }

    try
    {
        var raw = File.ReadAllText(path);
        if (string.IsNullOrWhiteSpace(raw))
        {
            return new RuntimeConfig();
        }

        return JsonSerializer.Deserialize<RuntimeConfig>(raw, JsonOptions.File) ?? new RuntimeConfig();
    }
    catch (Exception ex)
    {
        logger.LogWarning(ex, "Failed to read runtime config from {ConfigPath}.", path);
        return new RuntimeConfig();
    }
}

static string? ResolveApiBaseUrl()
{
    var explicitValue = GetEnv("CRONIQ_UI_API_BASEURL");
    if (explicitValue is not null)
    {
        return explicitValue;
    }

    var port = GetEnv("CRONIQ_UI_API_PORT");
    if (port is null)
    {
        return null;
    }

    var host = GetEnv("CRONIQ_UI_API_HOST");
    if (host is null)
    {
        return null;
    }
    var scheme = GetEnv("CRONIQ_UI_API_SCHEME") ?? "http";
    return $"{scheme}://{host}:{port}";
}

static string? ResolveSwaggerUiUrl()
{
    return GetEnv("CRONIQ_UI_SWAGGER_UI_URL", "CRONIQ_UI_SWAGGER_URL");
}

static string? ResolveDefaultTenantId()
{
    return GetEnv("CRONIQ_UI_DEFAULT_TENANT_ID");
}

static string? ResolveWebhooksActivityStreamMode()
{
    return GetEnv("CRONIQ_UI_WEBHOOKS_ACTIVITY_STREAM_MODE");
}

static string? ResolveWebhooksActivityGrpcBaseUrl()
{
    return GetEnv("CRONIQ_UI_WEBHOOKS_ACTIVITY_GRPC_BASEURL");
}

static string? ResolveWebhooksActivitySseBaseUrl()
{
    return GetEnv("CRONIQ_UI_WEBHOOKS_ACTIVITY_SSE_BASEURL");
}

static string? ResolveRunnersPresenceStreamMode()
{
    return GetEnv("CRONIQ_UI_RUNNERS_PRESENCE_STREAM_MODE");
}

static string? ResolveRunnersPresenceGrpcBaseUrl()
{
    return GetEnv("CRONIQ_UI_RUNNERS_PRESENCE_GRPC_BASEURL");
}

static string? ResolveRunnersPresenceSseBaseUrl()
{
    return GetEnv("CRONIQ_UI_RUNNERS_PRESENCE_SSE_BASEURL");
}

static string? GetEnv(params string[] keys)
{
    foreach (var key in keys)
    {
        var value = Normalize(Environment.GetEnvironmentVariable(key));
        if (value is not null)
        {
            return value;
        }
    }

    return null;
}

static string? Normalize(string? value)
{
    return string.IsNullOrWhiteSpace(value) ? null : value.Trim();
}

static string? ResolveBrowserWebRootPath(string[] args)
{
    var contentRoot = ResolveContentRootPath(args);
    var webRoot = ResolveWebRootPath(args, contentRoot);
    var browserWebRoot = Path.Combine(webRoot, "browser");
    return File.Exists(Path.Combine(browserWebRoot, "index.html")) ? browserWebRoot : null;
}

static string ResolveContentRootPath(string[] args)
{
    var fromArgs = GetArgValue(args, "--contentroot", "--contentRoot");
    if (!string.IsNullOrWhiteSpace(fromArgs))
    {
        return fromArgs;
    }

    var fromEnv = GetEnv("ASPNETCORE_CONTENTROOT");
    return string.IsNullOrWhiteSpace(fromEnv) ? Directory.GetCurrentDirectory() : fromEnv;
}

static string ResolveWebRootPath(string[] args, string contentRoot)
{
    var fromArgs = GetArgValue(args, "--webroot", "--webRoot");
    var fromEnv = string.IsNullOrWhiteSpace(fromArgs) ? GetEnv("ASPNETCORE_WEBROOT") : null;
    var webRoot = string.IsNullOrWhiteSpace(fromArgs) ? fromEnv : fromArgs;
    if (string.IsNullOrWhiteSpace(webRoot))
    {
        webRoot = "wwwroot";
    }

    return Path.IsPathRooted(webRoot) ? webRoot : Path.Combine(contentRoot, webRoot);
}

static string? GetArgValue(string[] args, params string[] keys)
{
    for (var i = 0; i < args.Length; i++)
    {
        var arg = args[i];
        foreach (var key in keys)
        {
            if (string.Equals(arg, key, StringComparison.OrdinalIgnoreCase))
            {
                return i + 1 < args.Length ? args[i + 1] : null;
            }

            var prefix = key + "=";
            if (arg.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            {
                return arg.Substring(prefix.Length);
            }
        }
    }

    return null;
}

static WebhooksRuntimeConfig? MergeWebhooksConfig(
    WebhooksRuntimeConfig? current,
    string? mode,
    string? grpcBaseUrl,
    string? sseBaseUrl)
{
    if (mode is null && grpcBaseUrl is null && sseBaseUrl is null)
    {
        return current;
    }

    var currentActivity = current?.ActivityStream;
    return new WebhooksRuntimeConfig
    {
        ActivityStream = new WebhookActivityStreamRuntimeConfig
        {
            Mode = mode ?? currentActivity?.Mode,
            GrpcBaseUrl = grpcBaseUrl ?? currentActivity?.GrpcBaseUrl,
            SseBaseUrl = sseBaseUrl ?? currentActivity?.SseBaseUrl
        }
    };
}

static RunnersRuntimeConfig? MergeRunnersConfig(
    RunnersRuntimeConfig? current,
    string? mode,
    string? grpcBaseUrl,
    string? sseBaseUrl)
{
    if (mode is null && grpcBaseUrl is null && sseBaseUrl is null)
    {
        return current;
    }

    var currentPresence = current?.PresenceStream;
    return new RunnersRuntimeConfig
    {
        PresenceStream = new RunnerPresenceStreamRuntimeConfig
        {
            Mode = mode ?? currentPresence?.Mode,
            GrpcBaseUrl = grpcBaseUrl ?? currentPresence?.GrpcBaseUrl,
            SseBaseUrl = sseBaseUrl ?? currentPresence?.SseBaseUrl
        }
    };
}

static class JsonOptions
{
    internal static readonly JsonSerializerOptions File = new()
    {
        PropertyNameCaseInsensitive = true
    };

    internal static readonly JsonSerializerOptions Output = new()
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };
}

internal sealed record WebhooksRuntimeConfig
{
    [JsonPropertyName("activityStream")]
    public WebhookActivityStreamRuntimeConfig? ActivityStream { get; init; }
}

internal sealed record RunnersRuntimeConfig
{
    [JsonPropertyName("presenceStream")]
    public RunnerPresenceStreamRuntimeConfig? PresenceStream { get; init; }
}

internal sealed record WebhookActivityStreamRuntimeConfig
{
    [JsonPropertyName("mode")]
    public string? Mode { get; init; }

    [JsonPropertyName("grpcBaseUrl")]
    public string? GrpcBaseUrl { get; init; }

    [JsonPropertyName("sseBaseUrl")]
    public string? SseBaseUrl { get; init; }
}

internal sealed record RunnerPresenceStreamRuntimeConfig
{
    [JsonPropertyName("mode")]
    public string? Mode { get; init; }

    [JsonPropertyName("grpcBaseUrl")]
    public string? GrpcBaseUrl { get; init; }

    [JsonPropertyName("sseBaseUrl")]
    public string? SseBaseUrl { get; init; }
}

internal sealed record RuntimeConfig
{
    [JsonPropertyName("apiBaseUrl")]
    public string? ApiBaseUrl { get; init; }

    [JsonPropertyName("swaggerUiUrl")]
    public string? SwaggerUiUrl { get; init; }

    [JsonPropertyName("grafanaUrl")]
    public string? GrafanaUrl { get; init; }

    [JsonPropertyName("defaultTenantId")]
    public string? DefaultTenantId { get; init; }

    [JsonPropertyName("webhooks")]
    public WebhooksRuntimeConfig? Webhooks { get; init; }

    [JsonPropertyName("runners")]
    public RunnersRuntimeConfig? Runners { get; init; }
}
