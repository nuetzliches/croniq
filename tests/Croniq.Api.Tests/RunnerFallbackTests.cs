using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Croniq.Rpc;
using Grpc.Core;
using Grpc.Net.Client;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class RunnerFallbackTests
{
    [Fact]
    public async Task Fallback_WhenGrpcUnavailable_UsesPolling()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(RunnerFallbackTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_runner_fallback";
        var tenantId = "00000000-0000-0000-0000-000000000011";
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

        var pipeline = new RecordingJobExecutionPipeline();
        var registry = new FakeJobRegistry();
        var policies = new FakePolicyResolver();
        var store = new NoopJobPersistenceProvider();

        builder.Services.AddSingleton<IJobExecutionPipeline>(pipeline);
        builder.Services.AddSingleton<IJobRegistry>(registry);
        builder.Services.AddSingleton<IPolicyResolver>(policies);
        builder.Services.AddSingleton<IJobPersistenceProvider>(store);
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        await using var app = builder.Build();
        app.UseCroniqApi();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:runner-fallback";
        await store.UpsertJobAsync(new JobDefinition(jobKey, "ops", "fallback", Variant: null, Description: null, Metadata: null, AssignedRunnerId: "default"), scope, CancellationToken.None);

        var triggerId = $"{jobKey}:once-{Guid.NewGuid():N}";
        var trigger = new TriggerDefinition(
            triggerId,
            jobKey,
            TriggerSchedule.OnceExpression,
            scope,
            StartAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            EndAtUtc: null,
            Enabled: true,
            Metadata: null,
            TimeZoneId: TimeZoneInfo.Utc.Id);
        await store.UpsertTriggerAsync(trigger, CancellationToken.None);

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        using var grpcHttpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        grpcHttpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = grpcHttpClient });
        var runnerClient = new Runner.RunnerClient(channel);

        var grpcFailed = false;
        try
        {
            using var call = runnerClient.Connect();
            await call.RequestStream.WriteAsync(new RunnerMessage
            {
                Hello = new RunnerHello
                {
                    RunnerId = "default",
                    MaxInflight = 1
                }
            });
            await call.ResponseStream.MoveNext(CancellationToken.None);
        }
        catch (RpcException)
        {
            grpcFailed = true;
        }
        catch (HttpProtocolException)
        {
            grpcFailed = true;
        }
        catch (HttpRequestException)
        {
            grpcFailed = true;
        }

        grpcFailed.ShouldBeTrue();

        using var httpClient = new HttpClient { BaseAddress = new Uri(address) };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        var poll = new WorkPollRequest(
            EnvironmentTag: environmentTag,
            RunnerId: "default",
            BatchSize: 1);

        var pollResponse = await httpClient.PostAsJsonAsync($"/tenants/{tenantId}/work/poll", poll);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        payload.ShouldNotBeNull();
        payload.Leases.Length.ShouldBe(1);

        await app.StopAsync();
    }
}
