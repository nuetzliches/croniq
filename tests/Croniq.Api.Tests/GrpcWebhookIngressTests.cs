using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
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

public sealed class GrpcWebhookIngressTests
{
    [Fact]
    public async Task WebhookIngressGrpc_Connect_ReturnsServerHello()
    {
        var apiKey = "ak_webhook_ingress_hello";
        var tenantId = "tenant-ingress-hello";
        var environmentTag = "dev";
        var caller = CreateCaller(tenantId, environmentTag, new[] { CroniqScopes.WebhooksIngress });

        var store = new InMemoryWebhookIngressEventStore();
        var callerFactory = CreateCallerFactory(apiKey, caller);

        var builder = CreateBuilder(apiKey, tenantId, environmentTag, callerFactory, store);
        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookIngressGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var (channel, client, httpClient) = CreateClient(address, apiKey);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = "consumer-1",
                MaxInflight = 1
            }
        });

        (await call.ResponseStream.MoveNext(CancellationToken.None)).ShouldBeTrue();
        var response = call.ResponseStream.Current;
        response.ShouldNotBeNull();
        response.Hello.ShouldNotBeNull();
        response.Hello.TenantId.ShouldBe(tenantId);
        response.Hello.EnvironmentTag.ShouldBe(environmentTag);
        response.Hello.ServerId.ShouldNotBeNullOrWhiteSpace();

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();

        channel.Dispose();
        httpClient.Dispose();
    }

    [Fact]
    public async Task WebhookIngressGrpc_Connect_RejectsTenantMismatch()
    {
        var apiKey = "ak_webhook_ingress_mismatch";
        var tenantId = "tenant-ingress-a";
        var environmentTag = "dev";
        var caller = CreateCaller(tenantId, environmentTag, new[] { CroniqScopes.WebhooksIngress });

        var store = new InMemoryWebhookIngressEventStore();
        var callerFactory = CreateCallerFactory(apiKey, caller);

        var builder = CreateBuilder(apiKey, tenantId, environmentTag, callerFactory, store);
        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookIngressGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var (channel, client, httpClient) = CreateClient(address, apiKey);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                TenantId = "tenant-ingress-b",
                EnvironmentTag = environmentTag,
                ConsumerId = "consumer-1",
                MaxInflight = 1
            }
        });

        var ex = await Should.ThrowAsync<RpcException>(async () =>
            await call.ResponseStream.MoveNext(CancellationToken.None));
        ex.StatusCode.ShouldBe(StatusCode.PermissionDenied);

        await app.StopAsync();

        channel.Dispose();
        httpClient.Dispose();
    }

    [Fact]
    public async Task WebhookIngressGrpc_Connect_RejectsMissingScope()
    {
        var apiKey = "ak_webhook_ingress_scope";
        var tenantId = "tenant-ingress-scope";
        var environmentTag = "dev";
        var caller = CreateCaller(tenantId, environmentTag, Array.Empty<string>());

        var store = new InMemoryWebhookIngressEventStore();
        var callerFactory = CreateCallerFactory(apiKey, caller);

        var builder = CreateBuilder(apiKey, tenantId, environmentTag, callerFactory, store);
        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookIngressGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var (channel, client, httpClient) = CreateClient(address, apiKey);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = "consumer-1",
                MaxInflight = 1
            }
        });

        var ex = await Should.ThrowAsync<RpcException>(async () =>
            await call.ResponseStream.MoveNext(CancellationToken.None));
        ex.StatusCode.ShouldBe(StatusCode.PermissionDenied);

        await app.StopAsync();

        channel.Dispose();
        httpClient.Dispose();
    }

    [Fact]
    public async Task WebhookIngressGrpc_Assigns_Extends_And_Acks_Event()
    {
        var apiKey = "ak_webhook_ingress_ack";
        var tenantId = "tenant-ingress-ack";
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

        var (channel, client, httpClient) = CreateClient(address, apiKey);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = "consumer-1",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForEventAsync(call.ResponseStream, TimeSpan.FromSeconds(3));
        assigned.EventId.ShouldBe(ingress.EventId);
        assigned.LeaseId.ShouldNotBeNullOrWhiteSpace();

        var requestedExpiry = DateTimeOffset.UtcNow.AddSeconds(30);
        var expectedExpiry = DateTimeOffset.FromUnixTimeMilliseconds(requestedExpiry.ToUnixTimeMilliseconds());

        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Extend = new WebhookEventExtend
            {
                EventId = assigned.EventId,
                LeaseId = assigned.LeaseId,
                LeaseExpiresAtUtc = expectedExpiry.ToUnixTimeMilliseconds()
            }
        });

        await WaitForLeaseExpiryAsync(store, assigned.EventId, expectedExpiry);

        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Ack = new WebhookEventAck
            {
                EventId = assigned.EventId,
                LeaseId = assigned.LeaseId,
                Succeeded = true
            }
        });

        await WaitForStatusAsync(store, assigned.EventId, InMemoryWebhookIngressEventStore.StatusDelivered);

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();

        channel.Dispose();
        httpClient.Dispose();
    }

    [Fact]
    public async Task WebhookIngressGrpc_Nack_Requeues_Event()
    {
        var apiKey = "ak_webhook_ingress_nack";
        var tenantId = "tenant-ingress-nack";
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

        var (channel, client, httpClient) = CreateClient(address, apiKey);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = "consumer-1",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForEventAsync(call.ResponseStream, TimeSpan.FromSeconds(3));
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Nack = new WebhookEventNack
            {
                EventId = assigned.EventId,
                LeaseId = assigned.LeaseId,
                Reason = "retry"
            }
        });

        await WaitForStatusAsync(store, assigned.EventId, InMemoryWebhookIngressEventStore.StatusPending);

        var reassigned = await WaitForEventAsync(call.ResponseStream, TimeSpan.FromSeconds(3));
        reassigned.EventId.ShouldBe(assigned.EventId);
        reassigned.LeaseId.ShouldNotBe(assigned.LeaseId);

        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Ack = new WebhookEventAck
            {
                EventId = reassigned.EventId,
                LeaseId = reassigned.LeaseId,
                Succeeded = true
            }
        });

        await WaitForStatusAsync(store, assigned.EventId, InMemoryWebhookIngressEventStore.StatusDelivered);

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();

        channel.Dispose();
        httpClient.Dispose();
    }

    [Fact]
    public async Task WebhookIngressGrpc_LeaseExpiry_Reassigns_Event()
    {
        var apiKey = "ak_webhook_ingress_expiry";
        var tenantId = "tenant-ingress-expiry";
        var environmentTag = "dev";
        var caller = CreateCaller(tenantId, environmentTag, new[] { CroniqScopes.WebhooksIngress });

        var store = new InMemoryWebhookIngressEventStore
        {
            LeaseDurationOverride = TimeSpan.FromMilliseconds(250)
        };
        var callerFactory = CreateCallerFactory(apiKey, caller);

        var builder = CreateBuilder(apiKey, tenantId, environmentTag, callerFactory, store);
        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookIngressGrpc();

        var ingress = CreateIngressEvent(tenantId, environmentTag);
        await store.EnqueueAsync(ingress, CancellationToken.None);

        await app.StartAsync();
        var address = app.Urls.First();

        var (channel, client, httpClient) = CreateClient(address, apiKey);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Hello = new WebhookConsumerHello
            {
                ConsumerId = "consumer-1",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForEventAsync(call.ResponseStream, TimeSpan.FromSeconds(2));
        var reassigned = await WaitForEventAsync(call.ResponseStream, TimeSpan.FromSeconds(2));
        reassigned.EventId.ShouldBe(assigned.EventId);
        reassigned.LeaseId.ShouldNotBe(assigned.LeaseId);

        await call.RequestStream.WriteAsync(new WebhookIngressClientMessage
        {
            Ack = new WebhookEventAck
            {
                EventId = reassigned.EventId,
                LeaseId = reassigned.LeaseId,
                Succeeded = true
            }
        });

        await WaitForStatusAsync(store, assigned.EventId, InMemoryWebhookIngressEventStore.StatusDelivered);

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();

        channel.Dispose();
        httpClient.Dispose();
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
            ApplicationName = typeof(GrpcWebhookIngressTests).Assembly.FullName,
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
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        return builder;
    }

    private static (GrpcChannel Channel, WebhookIngress.WebhookIngressClient Client, HttpClient HttpClient) CreateClient(
        string address,
        string apiKey)
    {
        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new WebhookIngress.WebhookIngressClient(channel);
        return (channel, client, httpClient);
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

    private static async Task<WebhookIngressEvent> WaitForEventAsync(
        IAsyncStreamReader<WebhookIngressServerMessage> responseStream,
        TimeSpan timeout)
    {
        using var cts = new CancellationTokenSource(timeout);
        while (await responseStream.MoveNext(cts.Token))
        {
            var current = responseStream.Current;
            if (current?.Event is not null)
            {
                return current.Event;
            }
        }

        throw new InvalidOperationException("No webhook ingress event received.");
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
