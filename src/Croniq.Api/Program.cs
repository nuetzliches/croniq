using System.Diagnostics;
using System.Threading.RateLimiting;
using Croniq.Api;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Auth.Xtraq;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Xtraq;
using Croniq.Providers.Default;
using Croniq.Sdk;
using Microsoft.AspNetCore.RateLimiting;
using Microsoft.Extensions.Options;

var builder = WebApplication.CreateBuilder(args);

builder.Services.Configure<CroniqApiOptions>(builder.Configuration.GetSection("Croniq:Api"));
builder.Services.Configure<CroniqOptions>(builder.Configuration.GetSection("Croniq:Core"));

builder.Services.AddCroniqCore();
builder.Services.AddCroniqDefaultProviders();
builder.Services.AddCroniqInMemoryJobStore();

var apiOpts = builder.Configuration.GetSection("Croniq:Api").Get<CroniqApiOptions>() ?? new CroniqApiOptions();
var coreOpts = builder.Configuration.GetSection("Croniq:Core").Get<CroniqOptions>() ?? new CroniqOptions();

var authConnectionString =
    builder.Configuration.GetSection("Croniq:Auth:ConnectionString").Value ??
    builder.Configuration.GetConnectionString("CroniqAuth") ??
    builder.Configuration.GetConnectionString("Croniq") ??
    builder.Configuration.GetConnectionString("DefaultConnection");

if (string.IsNullOrWhiteSpace(authConnectionString))
{
    throw new InvalidOperationException("Croniq Auth connection string is required (Croniq:Auth:ConnectionString or connection string CroniqAuth/Croniq/DefaultConnection).");
}

builder.Services.AddXtraqDbContext(options =>
{
    options.ConnectionString = authConnectionString;
});
builder.Services.AddCroniqAuthXtraq();
builder.Services.AddSingleton<ICallerContextAccessor, CallerContextAccessor>();
builder.Services.AddSingleton<ICallerContextFactory, CallerContextFactory>();

if (apiOpts.RequestsPerMinute > 0)
{
    builder.Services.AddRateLimiter(options =>
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
}

var app = builder.Build();

if (apiOpts.RequestsPerMinute > 0)
{
    app.UseRateLimiter();
}
app.Use(async (context, next) =>
{
    var apiOptions = context.RequestServices.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
    var callerAccessor = context.RequestServices.GetRequiredService<ICallerContextAccessor>();
    var callerFactory = context.RequestServices.GetRequiredService<ICallerContextFactory>();

    if (!string.IsNullOrWhiteSpace(apiOptions.ApiKey))
    {
        var provided = context.Request.Headers["X-Croniq-Key"].FirstOrDefault();
        if (string.IsNullOrWhiteSpace(provided))
        {
            context.Response.StatusCode = StatusCodes.Status401Unauthorized;
            await context.Response.WriteAsync("missing api key");
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
    CancellationToken cancellationToken) =>
{
    if (!JobKey.TryParse(request.JobKey, out var jobKey) || !registry.TryGet(jobKey, out var descriptor))
    {
        return Results.NotFound(new { error = "job-not-registered", request.JobKey });
    }

    var metadata = ToReadOnly(request.Metadata) ?? new Dictionary<string, string>();
    var activitySource = new ActivitySource("Croniq.Api.Trigger");
    var execRequest = new JobExecutionRequest(jobKey, descriptor, metadata, activitySource);

    await pipeline.ExecuteAsync(execRequest, cancellationToken).ConfigureAwait(false);
    return Results.Accepted(value: new { status = "triggered", request.JobKey });
});

app.Run();

static (string TenantId, string EnvironmentTag, string NamespaceSegment, string JobName, string? Variant) ParseJobKey(string jobKey)
{
    if (!JobKey.TryParse(jobKey, out var parsed))
    {
        throw new ArgumentException($"Invalid JobKey format: {jobKey}", nameof(jobKey));
    }

    return (parsed.TenantId, parsed.EnvironmentTag, parsed.NamespaceSegment, parsed.JobName, parsed.Variant);
}

static IReadOnlyDictionary<string, string>? ToReadOnly(IDictionary<string, string>? source)
{
    if (source is null) return null;
    if (source is IReadOnlyDictionary<string, string> ro) return ro;
    return new Dictionary<string, string>(source);
}
