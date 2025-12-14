using System;
using System.Net;
using System.Net.Http;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Net.Client;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class GrpcE2eTests
{
    [Fact]
    public async Task Grpc_EndToEnd_Trigger_And_Schedule()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcE2eTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_e2e";
        var tenantId = "tenant-e2e";
        var environmentTag = "dev";

        builder.Configuration.AddInMemoryCollection(new Dictionary<string, string?>
        {
            ["Croniq:Api:RequestsPerMinute"] = "0",
            ["Croniq:Auth:Mode"] = "InMemory",
            ["Croniq:Auth:InMemory:ApiKey"] = apiKey,
            ["Croniq:Auth:InMemory:TenantId"] = tenantId,
            ["Croniq:Auth:InMemory:EnvironmentTag"] = environmentTag
        });

        builder.Services.AddCroniqApiServices(builder.Configuration);
        builder.Services.AddCroniqApiRateLimiter();
        builder.Services.AddLogging();
        builder.Services.AddGrpc();

        var pipeline = new RecordingJobExecutionPipeline();
        var registry = new FakeJobRegistry();
        var policies = new FakePolicyResolver();
        var store = new NoopJobPersistenceProvider();

        builder.Services.AddSingleton<IJobExecutionPipeline>(pipeline);
        builder.Services.AddSingleton<IJobRegistry>(registry);
        builder.Services.AddSingleton<IPolicyResolver>(policies);
        builder.Services.AddSingleton<IJobPersistenceProvider>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqSchedulerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Scheduler.SchedulerClient(channel);

        var jobKey = $"{tenantId}:{environmentTag}:ops:grpc-e2e";
        registry.EnsureJob(jobKey);

        var upsert = await client.UpsertScheduleAsync(new UpsertScheduleRequest
        {
            JobKey = jobKey,
            CronExpression = "0/5 * * * * ?",
            Description = "e2e"
        });

        upsert.JobKey.ShouldBe(jobKey);
        upsert.TriggerId.ShouldNotBeNullOrWhiteSpace();

        var trigger = await client.TriggerJobAsync(new TriggerJobRequest { JobKey = jobKey });
        trigger.Status.ShouldBe("triggered");
        pipeline.Executions.Count.ShouldBe(1);
        pipeline.Executions[0].JobKey.ShouldBe(JobKey.Create(tenantId, environmentTag, "ops", "grpc-e2e"));

        await app.StopAsync();
    }
}
