using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Hosting;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public sealed class CroniqRunnerHeartbeatHostedServiceTests
{
    [Fact]
    public async Task ExecuteAsync_records_runner_heartbeat()
    {
        var store = new TrackingRunnerStore();
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "tenant-a",
            EnvironmentTag = "dev",
            InstanceId = "runner-1"
        });
        var hostOptions = Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions
        {
            HeartbeatInterval = TimeSpan.FromMilliseconds(10)
        });
        var runnerOptions = Microsoft.Extensions.Options.Options.Create(new RunnerStoreOptions
        {
            OnlineTtl = TimeSpan.FromSeconds(2)
        });
        var startupOptions = Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" });

        var service = new CroniqRunnerHeartbeatHostedService(
            store,
            options,
            hostOptions,
            runnerOptions,
            startupOptions,
            NullLogger<CroniqRunnerHeartbeatHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        var heartbeat = await store.WaitForHeartbeatAsync(TimeSpan.FromSeconds(1));

        await service.StopAsync(CancellationToken.None);

        heartbeat.Scope.TenantId.ShouldBe("tenant-a");
        heartbeat.Scope.EnvironmentTag.ShouldBe("dev");
        heartbeat.RunnerId.ShouldBe("runner-1");
        heartbeat.SeenAtUtc.ShouldBeGreaterThan(DateTimeOffset.UtcNow.AddMinutes(-1));
        heartbeat.MetadataJson.ShouldNotBeNull();

        var metadata = JsonSerializer.Deserialize<RunnerMetadataPayload>(
            heartbeat.MetadataJson!,
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
        metadata.ShouldNotBeNull();
        metadata!.Kind.ShouldBe("worker");
        metadata.Hostname.ShouldNotBeNullOrWhiteSpace();
    }

    [Fact]
    public async Task ExecuteAsync_skips_heartbeats_in_validate_mode()
    {
        var store = new TrackingRunnerStore();
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "tenant-a",
            EnvironmentTag = "dev",
            InstanceId = "runner-1"
        });
        var hostOptions = Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions
        {
            HeartbeatInterval = TimeSpan.FromMilliseconds(10)
        });
        var runnerOptions = Microsoft.Extensions.Options.Options.Create(new RunnerStoreOptions
        {
            OnlineTtl = TimeSpan.FromSeconds(2)
        });
        var startupOptions = Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Validate" });

        var service = new CroniqRunnerHeartbeatHostedService(
            store,
            options,
            hostOptions,
            runnerOptions,
            startupOptions,
            NullLogger<CroniqRunnerHeartbeatHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await Task.Delay(50);

        store.Heartbeats.ShouldBeEmpty();

        await service.StopAsync(CancellationToken.None);
    }

    private sealed class TrackingRunnerStore : IRunnerStore
    {
        private readonly TaskCompletionSource<RunnerHeartbeat> _tcs =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public List<RunnerHeartbeat> Heartbeats { get; } = new();

        public Task UpsertHeartbeatAsync(RunnerHeartbeat heartbeat, CancellationToken cancellationToken)
        {
            Heartbeats.Add(heartbeat);
            _tcs.TrySetResult(heartbeat);
            return Task.CompletedTask;
        }

        public Task<IReadOnlyCollection<RunnerStatus>> ListAsync(RunnerQuery query, CancellationToken cancellationToken)
            => Task.FromResult<IReadOnlyCollection<RunnerStatus>>(Array.Empty<RunnerStatus>());

        public Task<RunnerHeartbeat> WaitForHeartbeatAsync(TimeSpan timeout) => _tcs.Task.WaitAsync(timeout);
    }

    private sealed record RunnerMetadataPayload(string Kind, string Hostname);
}
