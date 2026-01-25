using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
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

public sealed class GrpcWebhookActivityTests
{
    [Fact]
    public async Task WebhookActivityGrpc_Stream_EmitsUpdates()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcWebhookActivityTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_webhook_activity";
        var tenantId = "tenant-activity-grpc";
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
        builder.Services.AddSingleton<IWebhookActivityStore>(new StubWebhookActivityStore(tenantId, environmentTag));

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqWebhookActivityGrpc();

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
        var client = new WebhookActivity.WebhookActivityClient(channel);

        using var call = client.Stream(new WebhookActivityStreamRequest
        {
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            Limit = 1
        });

        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        (await call.ResponseStream.MoveNext(cts.Token)).ShouldBeTrue();
        var response = call.ResponseStream.Current;

        response.ShouldNotBeNull();
        response.Type.ShouldBe("activity.updated");
        response.EmittedAtUtc.ShouldBeGreaterThan(0);
        response.LatestOccurredAtUtc.ShouldBeGreaterThan(0);

        await app.StopAsync();
    }

    private sealed class StubWebhookActivityStore : IWebhookActivityStore
    {
        private readonly string _tenantId;
        private readonly string _environmentTag;

        public StubWebhookActivityStore(string tenantId, string environmentTag)
        {
            _tenantId = tenantId;
            _environmentTag = environmentTag;
        }

        public Task<IReadOnlyCollection<WebhookActivityEntry>> ListAsync(
            PartitionScope scope,
            WebhookActivityQuery query,
            CancellationToken cancellationToken)
        {
            var entry = new WebhookActivityEntry(
                Id: Guid.NewGuid().ToString("N"),
                Kind: WebhookActivityKind.Delivery,
                Status: WebhookActivityStatus.Success,
                HookKey: "hook-grpc",
                JobKey: "samples:grpc",
                TenantId: _tenantId,
                EnvironmentTag: _environmentTag,
                Source: WebhookActivitySources.Ingress,
                OccurredAtUtc: DateTimeOffset.UtcNow,
                LatencyMs: null,
                Attempts: 1,
                Reason: null,
                PayloadBytes: null,
                DeadLetterId: null);

            return Task.FromResult<IReadOnlyCollection<WebhookActivityEntry>>(new[] { entry });
        }

        public Task<WebhookActivitySummary> SummarizeAsync(
            PartitionScope scope,
            WebhookActivitySummaryQuery query,
            CancellationToken cancellationToken)
        {
            var bucketMinutes = query.BucketMinutes ?? WebhookActivitySummaryQuery.DefaultBucketMinutes;
            var windowStart = query.FromUtc ?? DateTimeOffset.UtcNow;
            var windowEnd = query.ToUtc ?? windowStart;
            return Task.FromResult(new WebhookActivitySummary(
                bucketMinutes,
                windowStart,
                windowEnd,
                Array.Empty<WebhookActivityBucket>()));
        }
    }
}
