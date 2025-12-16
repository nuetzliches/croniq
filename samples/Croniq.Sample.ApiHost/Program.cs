using Croniq.Api;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Sample.Jobs;
using Croniq.Webhooks;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

var otelBuilder = builder.Services.AddCroniqApiObservability(
    builder.Configuration,
    builder.Logging);

var corsPolicyName = "CroniqSampleApiCors";
var allowedOrigins = builder.Configuration
    .GetSection("Croniq:Sample:Api:Cors:AllowedOrigins")
    .Get<string[]>() ?? Array.Empty<string>();

builder.Services.AddCors(options =>
{
    options.AddPolicy(corsPolicyName, policy =>
    {
        if (allowedOrigins.Length == 0)
        {
            if (builder.Environment.IsDevelopment())
            {
                policy.AllowAnyOrigin().AllowAnyHeader().AllowAnyMethod();
                return;
            }

            throw new InvalidOperationException("Croniq:Sample:Api:Cors:AllowedOrigins must be configured outside Development.");
        }

        policy
            .WithOrigins(allowedOrigins)
            .AllowAnyHeader()
            .AllowAnyMethod();
    });
});

// Persist execution logs locally for the sample host; production can swap to object storage or disable this.
builder.Logging.AddCroniqExecutionLogSink();
builder.Services.AddCroniqFileExecutionLogStore();
builder.Services.Configure<ExecutionLogRetentionOptions>(builder.Configuration.GetSection("Croniq:Logging:Execution:Retention"));
builder.Services.AddHostedService<ExecutionLogRetentionService>();

builder.Services.AddCroniqWebhookObservability(
    builder.Configuration,
    builder.Logging,
    builder: otelBuilder);

builder.Services.AddCroniqSampleJobs();
builder.Services.AddCroniqApiSchemas();

var app = builder.Build();

app.UseCroniqApiSwaggerUi(builder.Configuration);

app.UseCors(corsPolicyName);
app.UseCroniqApi();
app.MapCroniqSchedulerGrpc();
app.UseCroniqWebhooks(mapHealthEndpoints: false);

app.Run();
