using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Persistence.Abstractions;
using PersistenceWebhookActivityBucket = Croniq.Persistence.Abstractions.WebhookActivityBucket;
using PersistenceWebhookActivitySummary = Croniq.Persistence.Abstractions.WebhookActivitySummary;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookActivityStreamTests
{
    [Fact]
    public async Task WebhookActivityStream_Emits_Sse_Data()
    {
        var apiKey = "ak_webhook_activity_stream";
        var tenantId = "tenant-activity";
        var environmentTag = "dev";
        var caller = CreateCaller(tenantId, environmentTag, new[] { CroniqScopes.WebhooksRead });

        var store = new StubWebhookActivityStore(new[]
        {
            new WebhookActivityEntry(
                Id: "evt-1",
                Kind: WebhookActivityKind.Delivery,
                Status: WebhookActivityStatus.Success,
                HookKey: "hook-a",
                JobKey: "jobs:demo",
                TenantId: tenantId,
                EnvironmentTag: environmentTag,
                Source: WebhookActivitySources.Ingress,
                OccurredAtUtc: DateTimeOffset.UtcNow,
                Reason: null,
                PayloadBytes: null,
                DeadLetterId: null)
        });

        var callerFactory = CreateCallerFactory(apiKey, caller);
        var builder = CreateBuilder(apiKey, tenantId, environmentTag, callerFactory, store);
        await using var app = builder.Build();
        app.UseCroniqApi();

        await app.StartAsync();
        var address = app.Urls.First();

        using var client = CreateClient(address, apiKey);

        var fromUtc = DateTimeOffset.UtcNow.AddMinutes(-5).ToString("O");
        using var response = await client.GetAsync(
            $"/tenants/{tenantId}/webhooks/activity/stream?environment={environmentTag}&fromUtc={Uri.EscapeDataString(fromUtc)}",
            HttpCompletionOption.ResponseHeadersRead);

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        response.Content.Headers.ContentType?.MediaType.ShouldBe("text/event-stream");

        await using var stream = await response.Content.ReadAsStreamAsync();
        var buffer = new byte[2048];
        var read = await stream.ReadAsync(buffer.AsMemory(0, buffer.Length), CancellationToken.None);

        read.ShouldBeGreaterThan(0);
        var payload = Encoding.UTF8.GetString(buffer, 0, read);
        payload.ShouldContain("data:");
    }

    private static WebApplicationBuilder CreateBuilder(
        string apiKey,
        string tenantId,
        string environmentTag,
        TestCallerContextFactory callerFactory,
        IWebhookActivityStore store)
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(WebhookActivityStreamTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

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
        builder.Services.AddSingleton<IWebhookActivityStore>(store);
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

    private sealed class StubWebhookActivityStore : IWebhookActivityStore
    {
        private readonly IReadOnlyCollection<WebhookActivityEntry> _entries;

        public StubWebhookActivityStore(IReadOnlyCollection<WebhookActivityEntry> entries)
        {
            _entries = entries;
        }

        public Task<IReadOnlyCollection<WebhookActivityEntry>> ListAsync(
            PartitionScope scope,
            WebhookActivityQuery query,
            CancellationToken cancellationToken)
        {
            var result = _entries
                .Where(entry => entry.TenantId == scope.TenantId && entry.EnvironmentTag == scope.EnvironmentTag)
                .Where(entry => !query.FromUtc.HasValue || entry.OccurredAtUtc >= query.FromUtc.Value)
                .Where(entry => !query.ToUtc.HasValue || entry.OccurredAtUtc <= query.ToUtc.Value)
                .Take(query.Limit)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<WebhookActivityEntry>>(result);
        }

        public Task<PersistenceWebhookActivitySummary> SummarizeAsync(
            PartitionScope scope,
            WebhookActivitySummaryQuery query,
            CancellationToken cancellationToken)
        {
            var bucketMinutes = query.BucketMinutes ?? WebhookActivitySummaryQuery.DefaultBucketMinutes;
            var windowStart = query.FromUtc ?? DateTimeOffset.UtcNow;
            var windowEnd = query.ToUtc ?? windowStart;
            return Task.FromResult(new PersistenceWebhookActivitySummary(
                bucketMinutes,
                windowStart,
                windowEnd,
                Array.Empty<PersistenceWebhookActivityBucket>()));
        }
    }
}
