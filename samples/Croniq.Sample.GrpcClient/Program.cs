using System;
using Croniq.Rpc;
using Microsoft.Extensions.DependencyInjection;

var endpoint = Environment.GetEnvironmentVariable("CRONIQ_ENDPOINT") ?? "http://localhost:5080";
var apiKey = Environment.GetEnvironmentVariable("CRONIQ_API_KEY") ?? "smoke-key";
var tenantId = Environment.GetEnvironmentVariable("CRONIQ_TENANT_ID") ?? "default";
var environmentTag = Environment.GetEnvironmentVariable("CRONIQ_ENVIRONMENT") ?? "dev";
var jobKey = Environment.GetEnvironmentVariable("CRONIQ_JOB_KEY") ?? "samples:grpc-demo";

var services = new ServiceCollection();
services.AddCroniqSchedulerClient(options =>
{
    options.Endpoint = endpoint;
    options.ApiKey = apiKey;
});
var provider = services.BuildServiceProvider();
var client = provider.GetRequiredService<Scheduler.SchedulerClient>();

Console.WriteLine($"Croniq gRPC demo -> {endpoint} (tenant {tenantId}/{environmentTag})");

var upsertRequest = new UpsertScheduleRequest
{
    JobKey = jobKey,
    CronExpression = "0/5 * * * * ?",
    Description = "grpc demo schedule"
};

try
{
    var upsert = await client.UpsertScheduleSafeAsync(upsertRequest);
    Console.WriteLine($"Upserted schedule: trigger={upsert.TriggerId}, job={upsert.JobKey}");

    var trigger = await client.TriggerJobSafeAsync(new TriggerJobRequest { JobKey = jobKey });
    Console.WriteLine($"Trigger status: {trigger.Status}");
}
catch (CroniqRpcException ex)
{
    Console.WriteLine($"gRPC error: {ex.StatusCode} - {ex.Detail}");
}
