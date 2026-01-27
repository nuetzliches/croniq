using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading;
using Croniq.Api.Models;
using System.Threading.Tasks;
using System.Text.Json;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
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
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class GrpcRunnerTests
{
    [Fact]
    public async Task RunnerGrpc_Connect_ReturnsServerHello()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner";
        var tenantId = "00000000-0000-0000-0000-000000000003";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

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
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        });

        (await call.ResponseStream.MoveNext(CancellationToken.None)).ShouldBeTrue();
        var response = call.ResponseStream.Current;
        response.ShouldNotBeNull();
        response.Hello.ShouldNotBeNull();
        response.Hello.TenantId.ShouldBe(tenantId);
        response.Hello.EnvironmentTag.ShouldBe(environmentTag);

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_WithRunnerInstanceCollision_ReturnsAlreadyExists()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_collision";
        var tenantId = "00000000-0000-0000-0000-000000000099";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

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
        var client = new Runner.RunnerClient(channel);

        using var firstCall = client.Connect();
        await firstCall.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                RunnerInstanceId = "instance-1",
                MaxInflight = 1
            }
        });

        (await firstCall.ResponseStream.MoveNext(CancellationToken.None)).ShouldBeTrue();

        using var secondCall = client.Connect();
        await secondCall.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                RunnerInstanceId = "instance-2",
                MaxInflight = 1
            }
        });

        var failure = await Should.ThrowAsync<RpcException>(async () =>
        {
            await secondCall.ResponseStream.MoveNext(CancellationToken.None);
        });
        failure.StatusCode.ShouldBe(StatusCode.AlreadyExists);
        failure.Status.Detail.ShouldBe("runner-id-in-use");

        await firstCall.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_Assigns_And_Acks_Work()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_assign";
        var tenantId = "00000000-0000-0000-0000-000000000004";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner";
        await store.UpsertJobAsync(new JobDefinition(jobKey, "ops", "grpc-runner", Variant: null, Description: null, Metadata: null), scope, CancellationToken.None);

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
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForAssignmentAsync(call.ResponseStream);
        assigned.ShouldNotBeNull();
        assigned.ExecutionId.ShouldNotBeNullOrWhiteSpace();
        assigned.LeaseId.ShouldNotBeNullOrWhiteSpace();

        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            AckSuccess = new WorkAckSuccess
            {
                ExecutionId = assigned.ExecutionId,
                LeaseId = assigned.LeaseId
            }
        });

        var remaining = await WaitForTriggersClearedAsync(store, scope);
        remaining.ShouldBeEmpty();

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_AckedWork_IsNotAvailableForPolling()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_parity";
        var tenantId = "00000000-0000-0000-0000-000000000009";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-parity";
        await store.UpsertJobAsync(new JobDefinition(jobKey, "ops", "grpc-runner-parity", Variant: null, Description: null, Metadata: null), scope, CancellationToken.None);

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
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForAssignmentAsync(call.ResponseStream);
        assigned.ShouldNotBeNull();

        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            AckSuccess = new WorkAckSuccess
            {
                ExecutionId = assigned.ExecutionId,
                LeaseId = assigned.LeaseId
            }
        });

        var pollRequest = new WorkPollRequest(environmentTag, "default", BatchSize: 1);
        var pollResponse = await httpClient.PostAsJsonAsync($"/tenants/{tenantId}/work/poll", pollRequest);
        pollResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var pollPayload = await pollResponse.Content.ReadFromJsonAsync<WorkPollResponse>();
        pollPayload.ShouldNotBeNull();
        pollPayload.Leases.ShouldBeEmpty();

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_RespectsAllowTestExecutions_AndReturnsIntent()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_test_gate";
        var tenantId = "00000000-0000-0000-0000-000000000008";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-test-gate";
        await store.UpsertJobAsync(
            new JobDefinition(jobKey, "ops", "grpc-runner-test-gate", Variant: null, Description: null, Metadata: null),
            scope,
            CancellationToken.None);

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
            TimeZoneId: TimeZoneInfo.Utc.Id,
            ExecutionMode: ExecutionIntent.ExecutionModes.Test,
            InvocationSource: ExecutionIntent.InvocationSources.Manual);
        await store.UpsertTriggerAsync(trigger, CancellationToken.None);

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using (var deniedCall = client.Connect())
        {
            await deniedCall.RequestStream.WriteAsync(new RunnerMessage
            {
                Hello = new RunnerHello
                {
                    RunnerId = "default",
                    MaxInflight = 1,
                    AllowTestExecutions = false
                }
            });

            var deniedAssignment = await TryWaitForAssignmentAsync(deniedCall.ResponseStream, TimeSpan.FromSeconds(1));
            deniedAssignment.ShouldBeNull();
        }

        using (var allowedCall = client.Connect())
        {
            await allowedCall.RequestStream.WriteAsync(new RunnerMessage
            {
                Hello = new RunnerHello
                {
                    RunnerId = "default",
                    MaxInflight = 1,
                    AllowTestExecutions = true
                }
            });

            var assigned = await WaitForAssignmentAsync(allowedCall.ResponseStream);
            assigned.ExecutionMode.ShouldBe(ExecutionIntent.ExecutionModes.Test);
            assigned.InvocationSource.ShouldBe(ExecutionIntent.InvocationSources.Manual);

            await allowedCall.RequestStream.WriteAsync(new RunnerMessage
            {
                AckSuccess = new WorkAckSuccess
                {
                    ExecutionId = assigned.ExecutionId,
                    LeaseId = assigned.LeaseId
                }
            });
        }

        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_Events_AppendsExecutionLogs()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_events";
        var tenantId = "00000000-0000-0000-0000-000000000009";
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
        var executionLogs = new TestExecutionLogReader();

        builder.Services.AddSingleton<IJobExecutionPipeline>(pipeline);
        builder.Services.AddSingleton<IJobRegistry>(registry);
        builder.Services.AddSingleton<IPolicyResolver>(policies);
        builder.Services.AddSingleton<IJobPersistenceProvider>(store);
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);
        builder.Services.AddSingleton(executionLogs);
        builder.Services.AddSingleton<IExecutionLogReader>(sp => sp.GetRequiredService<TestExecutionLogReader>());
        builder.Services.AddSingleton<IExecutionLogStore>(sp => sp.GetRequiredService<TestExecutionLogReader>());

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-events";
        await store.UpsertJobAsync(
            new JobDefinition(jobKey, "ops", "grpc-runner-events", Variant: null, Description: null, Metadata: null),
            scope,
            CancellationToken.None);

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
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForAssignmentAsync(call.ResponseStream);
        assigned.ShouldNotBeNull();

        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Events = new WorkEvents
            {
                ExecutionId = assigned.ExecutionId,
                LeaseId = assigned.LeaseId,
                Events =
                {
                    new WorkEvent
                    {
                        Message = "hello from runner",
                        Level = "Information",
                        EventType = "runner"
                    }
                }
            }
        });

        var hasMessage = await WaitForLogEntryAsync(
            executionLogs,
            assigned.ExecutionId,
            root =>
            {
                if (root.TryGetProperty("renderedMessage", out var rendered)
                    && rendered.GetString()?.Contains("hello from runner", StringComparison.OrdinalIgnoreCase) == true)
                {
                    return true;
                }

                if (root.TryGetProperty("messageTemplate", out var template)
                    && template.GetString()?.Contains("hello from runner", StringComparison.OrdinalIgnoreCase) == true)
                {
                    return true;
                }

                return false;
            });
        hasMessage.ShouldBeTrue();

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_AckTwice_IsIgnored()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_ack_twice";
        var tenantId = "00000000-0000-0000-0000-000000000010";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-ack-twice";
        await store.UpsertJobAsync(
            new JobDefinition(jobKey, "ops", "grpc-runner-ack-twice", Variant: null, Description: null, Metadata: null),
            scope,
            CancellationToken.None);

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
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForAssignmentAsync(call.ResponseStream);
        assigned.ShouldNotBeNull();

        var ack = new RunnerMessage
        {
            AckSuccess = new WorkAckSuccess
            {
                ExecutionId = assigned.ExecutionId,
                LeaseId = assigned.LeaseId
            }
        };
        await call.RequestStream.WriteAsync(ack);
        await call.RequestStream.WriteAsync(ack);

        var remaining = await WaitForTriggersClearedAsync(store, scope);
        remaining.ShouldBeEmpty();

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_RejectedTestExecution_StoresWarningLog()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_reject_test";
        var tenantId = "00000000-0000-0000-0000-000000000012";
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
        var executionLogs = new TestExecutionLogReader();

        builder.Services.AddSingleton<IJobExecutionPipeline>(pipeline);
        builder.Services.AddSingleton<IJobRegistry>(registry);
        builder.Services.AddSingleton<IPolicyResolver>(policies);
        builder.Services.AddSingleton<IJobPersistenceProvider>(store);
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);
        builder.Services.AddSingleton(executionLogs);
        builder.Services.AddSingleton<IExecutionLogReader>(sp => sp.GetRequiredService<TestExecutionLogReader>());
        builder.Services.AddSingleton<IExecutionLogStore>(sp => sp.GetRequiredService<TestExecutionLogReader>());

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-reject-test";
        await store.UpsertJobAsync(
            new JobDefinition(jobKey, "ops", "grpc-runner-reject-test", Variant: null, Description: null, Metadata: null),
            scope,
            CancellationToken.None);

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
            TimeZoneId: TimeZoneInfo.Utc.Id,
            ExecutionMode: ExecutionIntent.ExecutionModes.Test,
            InvocationSource: ExecutionIntent.InvocationSources.Manual);
        await store.UpsertTriggerAsync(trigger, CancellationToken.None);

        AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1,
                AllowTestExecutions = true
            }
        });

        var assigned = await WaitForAssignmentAsync(call.ResponseStream);
        assigned.ShouldNotBeNull();

        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            AckFailure = new WorkAckFailure
            {
                ExecutionId = assigned.ExecutionId,
                LeaseId = assigned.LeaseId,
                DeadLetterReason = WorkRejectionReasons.TestNotAllowed
            }
        });

        var hasWarning = await WaitForLogEntryAsync(
            executionLogs,
            assigned.ExecutionId,
            root =>
                root.TryGetProperty("properties", out var properties)
                && properties.TryGetProperty("croniq.warning.type", out var warningType)
                && warningType.GetString() == WorkRejectionReasons.TestNotAllowed);
        hasWarning.ShouldBeTrue();

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_AckFailure_WithNextFireTime_ReschedulesTrigger()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_retry";
        var tenantId = "00000000-0000-0000-0000-000000000007";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-retry";
        await store.UpsertJobAsync(new JobDefinition(jobKey, "ops", "grpc-runner-retry", Variant: null, Description: null, Metadata: null), scope, CancellationToken.None);

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
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        });

        var assigned = await WaitForAssignmentAsync(call.ResponseStream);
        assigned.ShouldNotBeNull();

        var retryAt = DateTimeOffset.UtcNow.AddMinutes(5);
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            AckFailure = new WorkAckFailure
            {
                ExecutionId = assigned.ExecutionId,
                LeaseId = assigned.LeaseId,
                DeadLetterReason = "retry",
                NextFireTimeUtc = retryAt.ToUnixTimeMilliseconds()
            }
        });

        var rescheduled = await WaitForTriggerRescheduleAsync(store, scope, retryAt);
        rescheduled.TriggerId.ShouldBe(triggerId);

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_Connect_RejectsRunnerMismatch()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_mismatch";
        var tenantId = "00000000-0000-0000-0000-000000000005";
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

        var store = new NoopJobPersistenceProvider();
        builder.Services.AddSingleton<IJobPersistenceProvider>(store);
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

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
        var client = new Runner.RunnerClient(channel);

        using var call = client.Connect();
        await call.RequestStream.WriteAsync(new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "runner-1",
                MaxInflight = 1
            }
        });

        var ex = await Should.ThrowAsync<RpcException>(async () =>
            await call.ResponseStream.MoveNext(CancellationToken.None));
        ex.StatusCode.ShouldBe(StatusCode.PermissionDenied);

        await call.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    [Fact]
    public async Task RunnerGrpc_DoesNotAssignSameLeaseToSecondConnection()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(GrpcRunnerTests).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        var apiKey = "ak_grpc_runner_double";
        var tenantId = "00000000-0000-0000-0000-000000000006";
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
        builder.Services.AddSingleton<ICalendarStore>(store);
        builder.Services.AddSingleton<IJobStore>(store);
        builder.Services.AddSingleton<IPersistenceHealth>(store);

        builder.WebHost.ConfigureKestrel(options =>
        {
            options.Listen(IPAddress.Loopback, 0, listenOptions => listenOptions.Protocols = HttpProtocols.Http2);
        });

        await using var app = builder.Build();
        app.UseCroniqApi();
        app.MapCroniqRunnerGrpc();

        await app.StartAsync();
        var address = app.Urls.First();

        var scope = new PartitionScope(tenantId, environmentTag);
        const string jobKey = "ops:grpc-runner-double";
        await store.UpsertJobAsync(new JobDefinition(jobKey, "ops", "grpc-runner-double", Variant: null, Description: null, Metadata: null), scope, CancellationToken.None);

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
        var httpClient = new HttpClient
        {
            BaseAddress = new Uri(address),
            DefaultRequestVersion = new Version(2, 0),
            DefaultVersionPolicy = HttpVersionPolicy.RequestVersionOrHigher
        };
        httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

        using var channel = GrpcChannel.ForAddress(address, new GrpcChannelOptions { HttpClient = httpClient });
        var client = new Runner.RunnerClient(channel);

        using var callOne = client.Connect();
        using var callTwo = client.Connect();

        var hello = new RunnerMessage
        {
            Hello = new RunnerHello
            {
                RunnerId = "default",
                MaxInflight = 1
            }
        };

        await callOne.RequestStream.WriteAsync(hello);
        await callTwo.RequestStream.WriteAsync(hello);

        var assignOne = await TryWaitForAssignmentAsync(callOne.ResponseStream, TimeSpan.FromSeconds(2));
        var assignTwo = await TryWaitForAssignmentAsync(callTwo.ResponseStream, TimeSpan.FromSeconds(2));

        (assignOne is null ^ assignTwo is null).ShouldBeTrue();

        var assigned = assignOne ?? assignTwo;
        assigned.ShouldNotBeNull();

        var ackTarget = assignOne is null ? callTwo : callOne;
        await ackTarget.RequestStream.WriteAsync(new RunnerMessage
        {
            AckSuccess = new WorkAckSuccess
            {
                ExecutionId = assigned!.ExecutionId,
                LeaseId = assigned.LeaseId
            }
        });

        var remaining = await WaitForTriggersClearedAsync(store, scope);
        remaining.ShouldBeEmpty();

        await callOne.RequestStream.CompleteAsync();
        await callTwo.RequestStream.CompleteAsync();
        await app.StopAsync();
    }

    private static async Task<WorkAssigned> WaitForAssignmentAsync(IAsyncStreamReader<ServerMessage> responseStream)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        while (await responseStream.MoveNext(timeout.Token))
        {
            var current = responseStream.Current;
            if (current?.Assigned is not null)
            {
                return current.Assigned;
            }
        }

        throw new InvalidOperationException("No work assignment received.");
    }

    private static async Task<WorkAssigned?> TryWaitForAssignmentAsync(
        IAsyncStreamReader<ServerMessage> responseStream,
        TimeSpan timeout)
    {
        using var cts = new CancellationTokenSource(timeout);
        try
        {
            while (await responseStream.MoveNext(cts.Token))
            {
                var current = responseStream.Current;
                if (current?.Assigned is not null)
                {
                    return current.Assigned;
                }
            }
        }
        catch (OperationCanceledException)
        {
            return null;
        }
        catch (RpcException ex) when (ex.StatusCode == StatusCode.Cancelled || ex.StatusCode == StatusCode.DeadlineExceeded)
        {
            return null;
        }

        return null;
    }

    private static async Task<IReadOnlyCollection<TriggerDefinition>> WaitForTriggersClearedAsync(
        IJobPersistenceProvider store,
        PartitionScope scope)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var triggers = await store.ListTriggersAsync(scope, CancellationToken.None);
            if (triggers.Count == 0)
            {
                return triggers;
            }

            await Task.Delay(50);
        }

        return await store.ListTriggersAsync(scope, CancellationToken.None);
    }

    private static async Task<TriggerDefinition> WaitForTriggerRescheduleAsync(
        IJobPersistenceProvider store,
        PartitionScope scope,
        DateTimeOffset expectedAtUtc)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var triggers = await store.ListTriggersAsync(scope, CancellationToken.None);
            if (triggers.Count == 1)
            {
                var scheduled = triggers.First();
                if (scheduled.StartAtUtc is { } startAt
                    && startAt >= expectedAtUtc.AddSeconds(-1)
                    && startAt <= expectedAtUtc.AddSeconds(1))
                {
                    return scheduled;
                }
            }

            await Task.Delay(50);
        }

        var final = await store.ListTriggersAsync(scope, CancellationToken.None);
        return final.First();
    }

    private static async Task<IReadOnlyCollection<string>> WaitForLogLinesAsync(
        TestExecutionLogReader executionLogs,
        string executionId)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var lines = await ReadLogLinesAsync(executionLogs, executionId);
            if (lines.Count > 0)
            {
                return lines;
            }

            await Task.Delay(50);
        }

        return await ReadLogLinesAsync(executionLogs, executionId);
    }

    private static async Task<bool> WaitForLogEntryAsync(
        TestExecutionLogReader executionLogs,
        string executionId,
        Func<JsonElement, bool> predicate)
    {
        var deadline = DateTimeOffset.UtcNow.AddSeconds(2);
        while (DateTimeOffset.UtcNow < deadline)
        {
            var lines = await ReadLogLinesAsync(executionLogs, executionId);
            foreach (var line in lines)
            {
                using var doc = JsonDocument.Parse(line);
                if (predicate(doc.RootElement))
                {
                    return true;
                }
            }

            await Task.Delay(50);
        }

        return false;
    }

    private static async Task<List<string>> ReadLogLinesAsync(
        TestExecutionLogReader executionLogs,
        string executionId)
    {
        var lines = new List<string>();
        await foreach (var line in executionLogs.ReadLinesAsync(executionId, CancellationToken.None))
        {
            lines.Add(line);
        }

        return lines;
    }
}
