using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookIngressHttpTests
{
    [Fact]
    public async Task WebhookIngressHttp_Poll_Extend_And_Ack_Event()
    {
        var apiKey = "ak_webhook_ingress_http";
        var tenantId = "tenant-ingress-http";
        var environmentTag = "dev";
        var caller = CreateCaller(tenantId, environmentTag, new[] { CroniqScopes.WebhooksIngress });

        var store = new InMemoryWebhookIngressEventStore();
        var callerFactory = CreateCallerFactory(apiKey, caller);

        var builder = CreateBuilder(apiKey, tenantId, environmentTag, callerFactory, store);
        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookIngressGrpc();

        var ingress = CreateIngressEvent(tenantId, environmentTag);
        await store.EnqueueAsync(ingress, CancellationToken.None);

        await app.StartAsync();
        var address = app.Urls.First();

        using var client = CreateClient(address, apiKey);

        var poll = await client.GetFromJsonAsync<WebhookIngressPollResponse>(
            $"/tenants/{tenantId}/webhooks/ingress/poll?environment={environmentTag}&maxBatchSize=1");
        poll.ShouldNotBeNull();
        poll!.Events.Length.ShouldBe(1);

        var token = poll.Events[0];
        token.EventId.ShouldBe(ingress.EventId);

        var expiry = DateTimeOffset.UtcNow.AddSeconds(30);
        var extendRequest = new WebhookIngressExtendRequest(token.EventId, token.LeaseId, expiry.ToUnixTimeMilliseconds());
        var extendResponse = await client.PostAsJsonAsync(
            $"/tenants/{tenantId}/webhooks/ingress/extend?environment={environmentTag}",
            extendRequest);
        extendResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var extendPayload = await extendResponse.Content.ReadFromJsonAsync<WebhookIngressExtendResponse>();
        extendPayload.ShouldNotBeNull();
        extendPayload!.Extended.ShouldBeTrue();

        var expectedExpiry = DateTimeOffset.FromUnixTimeMilliseconds(extendRequest.LeaseExpiresAtUtc);
        await WaitForLeaseExpiryAsync(store, token.EventId, expectedExpiry);

        var ackRequest = new WebhookIngressAckRequest(token.EventId, token.LeaseId, true);
        var ackResponse = await client.PostAsJsonAsync(
            $"/tenants/{tenantId}/webhooks/ingress/ack?environment={environmentTag}",
            ackRequest);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        await WaitForStatusAsync(store, token.EventId, InMemoryWebhookIngressEventStore.StatusDelivered);

        await app.StopAsync();
    }

    private static WebApplicationBuilder CreateBuilder(
        string apiKey,
        string tenantId,
        string environmentTag,
        TestCallerContextFactory callerFactory,
        InMemoryWebhookIngressEventStore store)
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(WebhookIngressHttpTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        builder.Configuration.AddInMemoryCollection(new Dictionary<string, string?>
        {
            ["Croniq:Api:RequestsPerMinute"] = "0",
            ["Croniq:Auth:Mode"] = "InMemory",
            ["Croniq:Auth:InMemory:ApiKey"] = apiKey,
            ["Croniq:Auth:InMemory:TenantId"] = tenantId,
            ["Croniq:Auth:InMemory:EnvironmentTag"] = environmentTag,
            ["Croniq:Webhooks:Ingress:LeaseSeconds"] = "30",
            ["Croniq:Webhooks:Ingress:MaxBatchSize"] = "1",
            ["Croniq:Webhooks:Ingress:PollingIntervalMilliseconds"] = "100"
        });

        builder.Services.AddCroniqApiServices(builder.Configuration);
        builder.Services.AddCroniqApiRateLimiter();
        builder.Services.AddLogging();
        builder.Services.AddGrpc();
        builder.Services.AddSingleton<IWebhookIngressEventStore>(store);
        builder.Services.AddSingleton(callerFactory);
        builder.Services.AddSingleton<ICallerContextFactory>(sp => sp.GetRequiredService<TestCallerContextFactory>());

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http1AndHttp2);
        });

        return builder;
    }

    private static HttpClient CreateClient(string address, string apiKey)
    {
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address)
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);
        return httpClient;
    }

    private static TestCallerContextFactory CreateCallerFactory(string apiKey, ICallerContext caller)
    {
        var factory = new TestCallerContextFactory();
        factory.AddContext(apiKey, caller);
        return factory;
    }

    private static CallerContext CreateCaller(string tenantId, string environmentTag, IReadOnlyCollection<string> scopes)
    {
        return new CallerContext(
            tenantId,
            environmentTag,
            CallerType.ApiKey,
            CallerId: "itest-client",
            Scopes: scopes);
    }

    private static WebhookIngressEventCreate CreateIngressEvent(string tenantId, string environmentTag)
    {
        return new WebhookIngressEventCreate(
            EventId: $"evt_{Guid.NewGuid():N}",
            HookKey: "hook-ingress",
            JobKey: "ops:webhook-ingress",
            TenantId: tenantId,
            EnvironmentTag: environmentTag,
            Payload: "{\"hello\":\"world\"}",
            Headers: new Dictionary<string, string>
            {
                ["x-test"] = "true"
            },
            Metadata: new Dictionary<string, string>
            {
                ["source"] = "tests"
            },
            ReceivedAtUtc: DateTimeOffset.UtcNow);
    }

    private static async Task WaitForStatusAsync(
        InMemoryWebhookIngressEventStore store,
        string eventId,
        string expectedStatus)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
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

    private static async Task WaitForLeaseExpiryAsync(
        InMemoryWebhookIngressEventStore store,
        string eventId,
        DateTimeOffset expectedExpiry)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var snapshot = store.GetSnapshot(eventId);
            if (snapshot is not null && snapshot.LeaseExpiresAtUtc == expectedExpiry)
            {
                return;
            }

            await Task.Delay(25);
        }

        store.GetSnapshot(eventId)?.LeaseExpiresAtUtc.ShouldBe(expectedExpiry);
    }
}
