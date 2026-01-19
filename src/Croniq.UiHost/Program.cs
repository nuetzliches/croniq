using System.Text.Json;
using System.Text.Json.Serialization;
using Croniq.Core;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

var builder = WebApplication.CreateBuilder(args);

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

app.MapGet("/assets/croniq-config.json", async (HttpContext context, IWebHostEnvironment env) =>
{
    var config = LoadRuntimeConfig(env, app.Logger);
    context.Response.Headers.CacheControl = "no-store";
    await context.Response.WriteAsJsonAsync(config, JsonOptions.Output);
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

    return fromFile with
    {
        ApiBaseUrl = apiBaseUrl ?? fromFile.ApiBaseUrl,
        SwaggerUiUrl = swaggerUiUrl ?? fromFile.SwaggerUiUrl,
        DefaultTenantId = defaultTenantId ?? fromFile.DefaultTenantId
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

    var host = GetEnv("CRONIQ_UI_API_HOST") ?? "localhost";
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
}
