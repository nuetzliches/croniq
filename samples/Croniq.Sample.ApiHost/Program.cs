using System.Reflection;
using Croniq.Api;
using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Sample.Jobs;
using Croniq.Webhooks;
using Grpc.AspNetCore.Server;
using Microsoft.Extensions.Logging;

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
builder.Services.AddEndpointsApiExplorer();
builder.Services.AddSwaggerGen();
builder.Services.AddGrpcReflection();

var app = builder.Build();

var swaggerEnabled = app.Environment.IsDevelopment()
    || builder.Configuration.GetValue<bool>("Croniq:Api:ExposeSchemas");

if (swaggerEnabled)
{
    app.UseSwagger();
    app.UseSwaggerUI(options =>
    {
        options.SwaggerEndpoint("/swagger/v1/swagger.json", "Croniq Scheduler API v1");
        options.DisplayRequestDuration();
    });

    app.MapGrpcReflectionService();
}

app.UseCroniqApi();
app.UseCroniqWebhooks(mapHealthEndpoints: false);

var addresses = app.Urls?.Any() == true ? string.Join(", ", app.Urls) : "http://localhost:5000";
if (swaggerEnabled)
{
    var swaggerAddress = app.Urls?.FirstOrDefault() ?? "http://localhost:5000";
    app.Logger.LogInformation("Croniq API listening on {Addresses}. Swagger UI: {SwaggerUrl}", addresses, $"{swaggerAddress}/swagger");
}
else
{
    app.Logger.LogInformation("Croniq API listening on {Addresses}. Swagger UI disabled.", addresses);
}

app.Run();
