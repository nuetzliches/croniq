using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Security.Cryptography;
using System.Text;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class WebhookIngressTests
{
    private const string HookKey = "hook-ingress";
    private const string JobKeyValue = "ns:job";
    private const string TenantId = "tenant";
    private const string EnvironmentTag = "env";
    private const string Secret = "super-secret";

    [Fact]
    public async Task ValidSignature_ReturnsAccepted_AndExecutesPipeline()
    {
        var (client, pipeline) = CreateClient();
        var payload = "{\"hello\":\"world\"}";
        var signature = ComputeSignature(Secret, payload);

        var response = await client.PostAsync($"/tenants/{TenantId}/environments/{EnvironmentTag}/webhooks/{HookKey}", new StringContent(payload, Encoding.UTF8, "application/json")
        {
            Headers = { { "X-Croniq-Signature", signature } }
        });

        response.StatusCode.ShouldBe(HttpStatusCode.Accepted);
        pipeline.Executed.ShouldBeTrue();
    }

    [Fact]
    public async Task MissingSignature_Returns401()
    {
        var (client, pipeline) = CreateClient();
        var payload = "{}";

        var response = await client.PostAsJsonAsync($"/tenants/{TenantId}/environments/{EnvironmentTag}/webhooks/{HookKey}", payload);

        response.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);
        pipeline.Executed.ShouldBeFalse();
    }

    private static (HttpClient client, StubPipeline pipeline) CreateClient()
    {
        var builder = WebApplication.CreateBuilder();
        builder.WebHost.UseTestServer();

        var config = new Dictionary<string, string?>
        {
            ["Croniq:Auth:Mode"] = "InMemory",
            ["Croniq:Webhooks:Mode"] = "InMemory",
            ["Croniq:Webhooks:Security:AllowUnsignedHooks"] = "false",
            ["Croniq:Webhooks:Endpoints:0:HookKey"] = HookKey,
            ["Croniq:Webhooks:Endpoints:0:JobKey"] = JobKeyValue,
            ["Croniq:Webhooks:Endpoints:0:RequireSignature"] = "true",
            ["Croniq:Webhooks:Endpoints:0:Secret"] = Secret
        };
        builder.Configuration.AddInMemoryCollection(config);

        var registry = new StubRegistry();
        var pipeline = new StubPipeline();
        var policies = new StubPolicies();

        builder.Services.AddSingleton<IJobRegistry>(registry);
        builder.Services.AddSingleton<IJobExecutionPipeline>(pipeline);
        builder.Services.AddSingleton<IPolicyResolver>(policies);
        builder.Services.AddCroniqWebhookServices(builder.Configuration);
        builder.Services.AddCroniqWebhookRateLimiter();

        var app = builder.Build();
        app.UseRateLimiter();
        app.UseRouting();
        app.UseCroniqWebhooks();

        app.StartAsync().GetAwaiter().GetResult();
        return (app.GetTestClient(), pipeline);
    }

    private static string ComputeSignature(string secret, string payload)
    {
        var keyBytes = Encoding.UTF8.GetBytes(secret);
        var payloadBytes = Encoding.UTF8.GetBytes(payload ?? string.Empty);
        var hash = HMACSHA256.HashData(keyBytes, payloadBytes);
        return $"sha256={Convert.ToHexString(hash).ToLowerInvariant()}";
    }

    private sealed class StubRegistry : IJobRegistry
    {
        public IReadOnlyCollection<JobDescriptor> Descriptors => new[] { CreateDescriptor() };

        private static JobDescriptor CreateDescriptor()
        {
            JobKey.TryParse(JobKeyValue, out var jobKey);
            return new JobDescriptor(typeof(object), new Croniq.Sdk.CroniqJobAttribute("ns", "job"), jobKey);
        }

        public bool TryGet(JobKey jobKey, out JobDescriptor descriptor)
        {
            if (jobKey.Value == JobKeyValue)
            {
                descriptor = CreateDescriptor();
                return true;
            }

            descriptor = null!;
            return false;
        }
    }

    private sealed class StubPipeline : IJobExecutionPipeline
    {
        public bool Executed { get; private set; }
        public Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken)
        {
            Executed = true;
            return Task.CompletedTask;
        }
    }

    private sealed class StubPolicies : IPolicyResolver
    {
        public ExecutionPolicyOptions ResolveExecution(JobKey jobKey, PartitionScope? scope = null) => new();

        public MisfirePolicyOptions ResolveMisfire(JobKey jobKey, PartitionScope? scope = null) => new();

        public QuotaOptions ResolveQuota(JobKey jobKey, PartitionScope? scope = null) => new();
    }
}
