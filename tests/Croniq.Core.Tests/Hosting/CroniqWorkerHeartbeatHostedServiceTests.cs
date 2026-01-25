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

public sealed class CroniqWorkerHeartbeatHostedServiceTests
{
    [Fact]
    public async Task ExecuteAsync_records_worker_heartbeat()
    {
        var store = new TrackingWorkerStore();
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "tenant-a",
            EnvironmentTag = "dev",
            InstanceId = "worker-1"
        });
        var hostOptions = Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions
        {
            HeartbeatInterval = TimeSpan.FromMilliseconds(10)
        });
        var workerOptions = Microsoft.Extensions.Options.Options.Create(new WorkerStoreOptions
        {
            OnlineTtl = TimeSpan.FromSeconds(2)
        });
        var startupOptions = Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" });

        var service = new CroniqWorkerHeartbeatHostedService(
            store,
            options,
            hostOptions,
            workerOptions,
            startupOptions,
            dispatchStatusProvider: null,
            NullLogger<CroniqWorkerHeartbeatHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        var heartbeat = await store.WaitForHeartbeatAsync(TimeSpan.FromSeconds(1));

        await service.StopAsync(CancellationToken.None);

        heartbeat.Scope.TenantId.ShouldBe("tenant-a");
        heartbeat.Scope.EnvironmentTag.ShouldBe("dev");
        heartbeat.InstanceId.ShouldBe("worker-1");
        heartbeat.SeenAtUtc.ShouldBeGreaterThan(DateTimeOffset.UtcNow.AddMinutes(-1));
        heartbeat.MetadataJson.ShouldNotBeNull();

        var metadata = JsonSerializer.Deserialize<WorkerMetadataPayload>(
            heartbeat.MetadataJson!,
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
        metadata.ShouldNotBeNull();
        metadata!.Kind.ShouldBe("worker");
        metadata.Hostname.ShouldNotBeNullOrWhiteSpace();
    }

    [Fact]
    public async Task ExecuteAsync_skips_heartbeats_in_validate_mode()
    {
        var store = new TrackingWorkerStore();
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantId = "tenant-a",
            EnvironmentTag = "dev",
            InstanceId = "worker-1"
        });
        var hostOptions = Microsoft.Extensions.Options.Options.Create(new WorkerHostOptions
        {
            HeartbeatInterval = TimeSpan.FromMilliseconds(10)
        });
        var workerOptions = Microsoft.Extensions.Options.Options.Create(new WorkerStoreOptions
        {
            OnlineTtl = TimeSpan.FromSeconds(2)
        });
        var startupOptions = Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Validate" });

        var service = new CroniqWorkerHeartbeatHostedService(
            store,
            options,
            hostOptions,
            workerOptions,
            startupOptions,
            dispatchStatusProvider: null,
            NullLogger<CroniqWorkerHeartbeatHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await Task.Delay(50);

        store.Heartbeats.ShouldBeEmpty();

        await service.StopAsync(CancellationToken.None);
    }

    private sealed class TrackingWorkerStore : IWorkerStore
    {
        private readonly TaskCompletionSource<WorkerHeartbeat> _tcs =
            new(TaskCreationOptions.RunContinuationsAsynchronously);

        public List<WorkerHeartbeat> Heartbeats { get; } = new();

        public Task UpsertHeartbeatAsync(WorkerHeartbeat heartbeat, CancellationToken cancellationToken)
        {
            Heartbeats.Add(heartbeat);
            _tcs.TrySetResult(heartbeat);
            return Task.CompletedTask;
        }

        public Task<IReadOnlyCollection<WorkerStatus>> ListAsync(WorkerQuery query, CancellationToken cancellationToken)
            => Task.FromResult<IReadOnlyCollection<WorkerStatus>>(Array.Empty<WorkerStatus>());

        public Task<WorkerHeartbeat> WaitForHeartbeatAsync(TimeSpan timeout) => _tcs.Task.WaitAsync(timeout);
    }

    private sealed record WorkerMetadataPayload(string Kind, string Hostname);
}
