using System;
using System.Collections.Generic;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookIngressRelayE2eTests
{
    [Fact]
    public async Task RelayWorker_TriggersJob_And_AcksSuccess()
    {
        var tenantId = "tenant-relay-success";
        var environmentTag = "dmz";
        var dmzApiKey = "ak_dmz_relay_success";
        var jobKey = "ops:relay-success";
        var hookKey = "hook-relay-success";

        var store = new InMemoryWebhookIngressEventStore();
        await using var dmzApp = BuildDmzHost(tenantId, environmentTag, dmzApiKey, store);
        await dmzApp.StartAsync();
        var address = dmzApp.Urls.First();

        var pipeline = new RecordingJobExecutionPipeline();
        var registry = new FakeJobRegistry();
        var policies = new FakePolicyResolver();
        registry.EnsureJob(jobKey);

        using var relayHost = BuildRelayHost(address, tenantId, environmentTag, dmzApiKey, pipeline, registry, policies);
        await relayHost.StartAsync();

        try
        {
            var eventId = await EnqueueIngressEventAsync(store, tenantId, environmentTag, hookKey, jobKey);
            await WaitForExecutionAsync(pipeline, TimeSpan.FromSeconds(5));
            await WaitForStatusAsync(store, eventId, InMemoryWebhookIngressEventStore.StatusDelivered);

            pipeline.Executions.Count.ShouldBe(1);
            pipeline.Executions[0].JobKey.Value.ShouldBe(jobKey);
        }
        finally
        {
            await relayHost.StopAsync();
            await dmzApp.StopAsync();
        }
    }

    [Fact]
    public async Task RelayWorker_AcksFailure_WhenJobMissing()
    {
        var tenantId = "tenant-relay-failure";
        var environmentTag = "dmz";
        var dmzApiKey = "ak_dmz_relay_failure";
        var jobKey = "ops:relay-missing";
        var hookKey = "hook-relay-missing";

        var store = new InMemoryWebhookIngressEventStore();
        await using var dmzApp = BuildDmzHost(tenantId, environmentTag, dmzApiKey, store);
        await dmzApp.StartAsync();
        var address = dmzApp.Urls.First();

        var pipeline = new RecordingJobExecutionPipeline();
        var registry = new FakeJobRegistry();
        var policies = new FakePolicyResolver();

        using var relayHost = BuildRelayHost(address, tenantId, environmentTag, dmzApiKey, pipeline, registry, policies);
        await relayHost.StartAsync();

        try
        {
            var eventId = await EnqueueIngressEventAsync(store, tenantId, environmentTag, hookKey, jobKey);
            await WaitForStatusAsync(store, eventId, InMemoryWebhookIngressEventStore.StatusFailed);

            pipeline.Executions.Count.ShouldBe(0);
        }
        finally
        {
            await relayHost.StopAsync();
            await dmzApp.StopAsync();
        }
    }

    private static WebApplication BuildDmzHost(
        string tenantId,
        string environmentTag,
        string apiKey,
        InMemoryWebhookIngressEventStore store)
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(WebhookIngressRelayE2eTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        builder.Configuration.AddInMemoryCollection(new Dictionary<string, string?>
        {
            ["Croniq:Api:RequestsPerMinute"] = "0",
            ["Croniq:Auth:Mode"] = "InMemory",
            ["Croniq:Auth:InMemory:ApiKey"] = apiKey,
            ["Croniq:Auth:InMemory:TenantId"] = tenantId,
            ["Croniq:Auth:InMemory:EnvironmentTag"] = environmentTag,
            ["Croniq:Webhooks:Ingress:LeaseSeconds"] = "20",
            ["Croniq:Webhooks:Ingress:MaxBatchSize"] = "1",
            ["Croniq:Webhooks:Ingress:PollingIntervalMilliseconds"] = "100"
        });

        builder.Services.AddCroniqApiServices(builder.Configuration);
        builder.Services.AddCroniqApiRateLimiter();
        builder.Services.AddLogging();
        builder.Services.AddGrpc();
        builder.Services.AddSingleton<IWebhookIngressEventStore>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http1AndHttp2);
        });

        var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookIngressGrpc();
        return app;
    }

    private static IHost BuildRelayHost(
        string baseUrl,
        string tenantId,
        string environmentTag,
        string apiKey,
        RecordingJobExecutionPipeline pipeline,
        FakeJobRegistry registry,
        FakePolicyResolver policies)
    {
        return Host.CreateDefaultBuilder()
            .ConfigureAppConfiguration(config =>
            {
                config.AddInMemoryCollection(new Dictionary<string, string?>
                {
                    ["Croniq:Auth:Mode"] = "InMemory",
                    ["Croniq:Auth:InMemory:ApiKey"] = "relay-key",
                    ["Croniq:Auth:InMemory:TenantId"] = tenantId,
                    ["Croniq:Auth:InMemory:EnvironmentTag"] = environmentTag,
                    ["Croniq:Core:TenantId"] = tenantId,
                    ["Croniq:Core:EnvironmentTag"] = environmentTag,
                    ["Croniq:Core:InstanceId"] = "relay-itest",
                    ["Croniq:Webhooks:Mode"] = "Remote",
                    ["Croniq:Webhooks:Remote:BaseUrl"] = baseUrl,
                    ["Croniq:Webhooks:Remote:ApiKey"] = apiKey,
                    ["Croniq:Webhooks:Remote:StreamMode"] = "Polling",
                    ["Croniq:Webhooks:Remote:MaxInflight"] = "1",
                    ["Croniq:Webhooks:Remote:ReconnectDelaySeconds"] = "1",
                    ["Croniq:Webhooks:Remote:TimeoutSeconds"] = "5",
                    ["Croniq:Webhooks:Remote:EnableRelay"] = "true"
                });
            })
            .ConfigureServices((context, services) =>
            {
                services.AddCroniqWebhookServices(context.Configuration);
                services.AddLogging();
                services.AddSingleton<IJobExecutionPipeline>(pipeline);
                services.AddSingleton<IJobRegistry>(registry);
                services.AddSingleton<IPolicyResolver>(policies);
            })
            .Build();
    }

    private static async Task<string> EnqueueIngressEventAsync(
        IWebhookIngressEventStore store,
        string tenantId,
        string environmentTag,
        string hookKey,
        string jobKey)
    {
        var eventId = $"evt_{Guid.NewGuid():N}";
        await store.EnqueueAsync(new WebhookIngressEventCreate(
            eventId,
            hookKey,
            jobKey,
            tenantId,
            environmentTag,
            "{\"hello\":\"world\"}",
            Headers: null,
            Metadata: new Dictionary<string, string> { ["source"] = "relay-itest" },
            DateTimeOffset.UtcNow), CancellationToken.None);

        return eventId;
    }

    private static async Task WaitForExecutionAsync(
        RecordingJobExecutionPipeline pipeline,
        TimeSpan timeout)
    {
        var deadline = DateTimeOffset.UtcNow.Add(timeout);
        while (DateTimeOffset.UtcNow < deadline)
        {
            if (pipeline.Executions.Count > 0)
            {
                return;
            }

            await Task.Delay(50);
        }

        pipeline.Executions.Count.ShouldBeGreaterThan(0);
    }

    private static async Task WaitForStatusAsync(
        InMemoryWebhookIngressEventStore store,
        string eventId,
        string expectedStatus)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(5);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var snapshot = store.GetSnapshot(eventId);
            if (snapshot is not null && snapshot.Status == expectedStatus)
            {
                return;
            }

            await Task.Delay(50);
        }

        store.GetSnapshot(eventId)?.Status.ShouldBe(expectedStatus);
    }
}
