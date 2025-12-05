using System.Diagnostics;
using System.Threading.RateLimiting;
using Croniq.Api;
using Croniq.Api.Models;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
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

// Register Rate Limiter
builder.Services.AddRateLimiter(options =>
{
    options.RejectionStatusCode = StatusCodes.Status429TooManyRequests;
    options.GlobalLimiter = PartitionedRateLimiter.Create<HttpContext, string>(context =>
    {
        var apiOptions = context.RequestServices.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
        var key = context.Request.Headers["X-Croniq-Key"].FirstOrDefault() ?? "anonymous";
        var permits = Math.Max(1, apiOptions.RequestsPerMinute);

        return RateLimitPartition.GetFixedWindowLimiter(key, _ => new FixedWindowRateLimiterOptions
        {
            PermitLimit = permits,
            Window = TimeSpan.FromMinutes(1),
            QueueLimit = permits,
            QueueProcessingOrder = QueueProcessingOrder.OldestFirst
        });
    });
});

var app = builder.Build();

app.UseRateLimiter();
app.Use(async (context, next) =>
{
    var apiOptions = context.RequestServices.GetRequiredService<IOptions<CroniqApiOptions>>().Value;
    if (!string.IsNullOrWhiteSpace(apiOptions.ApiKey))
    {
        var provided = context.Request.Headers["X-Croniq-Key"].FirstOrDefault();
        if (!string.Equals(apiOptions.ApiKey, provided, StringComparison.Ordinal))
        {
            context.Response.StatusCode = StatusCodes.Status401Unauthorized;
            await context.Response.WriteAsync("invalid api key");
            return;
        }
    }

    await next().ConfigureAwait(false);
});

app.MapGet("/health", () => Results.Ok(new { status = "ok" }));

app.MapPost("/schedules", async (
    UpsertScheduleRequest request,
    IJobPersistenceProvider store,
    CancellationToken cancellationToken) =>
{
    var parts = ParseJobKey(request.JobKey);
    var triggerId = string.IsNullOrWhiteSpace(request.TriggerId)
        ? $"{request.JobKey}:{request.CronExpression}"
        : request.TriggerId;

    var scope = new PartitionScope(parts.TenantId, parts.EnvironmentTag);

    var job = new JobDefinition(
        request.JobKey,
        parts.NamespaceSegment,
        parts.JobName,
        parts.Variant,
        request.Description,
        request.Metadata);

    var trigger = new TriggerDefinition(
        triggerId,
        request.JobKey,
        request.CronExpression,
        scope,
        request.StartAtUtc,
        request.EndAtUtc,
        request.Enabled,
        request.Metadata);

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

    var metadata = request.Metadata ?? new Dictionary<string, string>();
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
