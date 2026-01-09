using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Hosting;

/// <summary>
/// Background service that emits runner heartbeats for worker availability.
/// </summary>
public sealed class CroniqRunnerHeartbeatHostedService : BackgroundService
{
    private static readonly JsonSerializerOptions MetadataSerializerOptions = new(JsonSerializerDefaults.Web);
    private static readonly TimeSpan MinHeartbeatSkew = TimeSpan.FromSeconds(1);

    private readonly IRunnerStore _runnerStore;
    private readonly CroniqOptions _coreOptions;
    private readonly WorkerHostOptions _hostOptions;
    private readonly RunnerStoreOptions _runnerOptions;
    private readonly CroniqStartupOptions _startupOptions;
    private readonly ILogger<CroniqRunnerHeartbeatHostedService> _logger;
    private readonly TimeSpan _heartbeatInterval;
    private readonly string? _metadataJson;

    public CroniqRunnerHeartbeatHostedService(
        IRunnerStore runnerStore,
        IOptions<CroniqOptions> options,
        IOptions<WorkerHostOptions> hostOptions,
        IOptions<RunnerStoreOptions> runnerOptions,
        IOptions<CroniqStartupOptions> startupOptions,
        ILogger<CroniqRunnerHeartbeatHostedService> logger)
    {
        _runnerStore = runnerStore ?? throw new ArgumentNullException(nameof(runnerStore));
        _coreOptions = options?.Value ?? throw new ArgumentNullException(nameof(options));
        _hostOptions = hostOptions?.Value ?? throw new ArgumentNullException(nameof(hostOptions));
        _runnerOptions = runnerOptions?.Value ?? new RunnerStoreOptions();
        _runnerOptions.Normalize();
        _startupOptions = startupOptions?.Value ?? new CroniqStartupOptions();
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _heartbeatInterval = ResolveInterval(_hostOptions.HeartbeatInterval, _runnerOptions.OnlineTtl);
        _metadataJson = BuildMetadataJson();
    }

    protected override async Task ExecuteAsync(CancellationToken stoppingToken)
    {
        var startupMode = CroniqStartupModeParser.Parse(_startupOptions.Mode);
        if (startupMode == CroniqStartupMode.Validate)
        {
            _logger.LogInformation("Croniq startup mode is Validate; runner heartbeats are disabled.");
            return;
        }

        if (_hostOptions.HeartbeatInterval <= TimeSpan.Zero)
        {
            _logger.LogInformation("Croniq runner heartbeats are disabled (HeartbeatInterval <= 0).");
            return;
        }

        var scope = new PartitionScope(_coreOptions.TenantId.Trim(), _coreOptions.EnvironmentTag);
        var runnerId = _coreOptions.InstanceId.Trim();
        using var logScope = _logger.BeginScope(new Dictionary<string, object?>
        {
            ["tenantId"] = scope.TenantId,
            ["environmentTag"] = scope.EnvironmentTag,
            ["runnerId"] = runnerId
        });

        if (_heartbeatInterval != _hostOptions.HeartbeatInterval)
        {
            _logger.LogWarning(
                "Heartbeat interval {Configured} exceeds online TTL {OnlineTtl}; clamped to {Effective}.",
                _hostOptions.HeartbeatInterval,
                _runnerOptions.OnlineTtl,
                _heartbeatInterval);
        }

        while (!stoppingToken.IsCancellationRequested)
        {
            try
            {
                var heartbeat = new RunnerHeartbeat(scope, runnerId, DateTimeOffset.UtcNow, _metadataJson);
                await _runnerStore.UpsertHeartbeatAsync(heartbeat, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "Failed to persist runner heartbeat.");
            }

            try
            {
                await Task.Delay(_heartbeatInterval, stoppingToken).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested)
            {
                break;
            }
        }
    }

    private static TimeSpan ResolveInterval(TimeSpan configured, TimeSpan onlineTtl)
    {
        if (configured <= TimeSpan.Zero || onlineTtl <= TimeSpan.Zero)
        {
            return configured;
        }

        var maxInterval = onlineTtl - MinHeartbeatSkew;
        if (maxInterval <= TimeSpan.Zero)
        {
            return configured;
        }

        return configured > maxInterval ? maxInterval : configured;
    }

    private static string? BuildMetadataJson()
    {
        var hostname = Environment.MachineName;
        if (string.IsNullOrWhiteSpace(hostname))
        {
            return null;
        }

        var metadata = new RunnerMetadata("worker", hostname);
        return JsonSerializer.Serialize(metadata, MetadataSerializerOptions);
    }

    private sealed record RunnerMetadata(string Kind, string Hostname);
}
