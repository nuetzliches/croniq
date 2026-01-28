using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using Microsoft.Extensions.Logging;

namespace Croniq.Runner;

public sealed record RunnerConfig
{
    public required string BaseUrl { get; init; }
    public required string TenantId { get; init; }
    public required string Environment { get; init; }
    public required string RunnerId { get; init; }
    public string? RunnerInstanceId { get; init; }
    public string? ApiKey { get; init; }
    public string? BearerToken { get; init; }
    public string? GrpcBaseUrl { get; init; }
    public TransportMode TransportMode { get; init; } = TransportMode.Auto;
    public bool AllowTestExecutions { get; init; }
    public int MaxInflight { get; init; } = 1;
    public string[]? Capabilities { get; init; }
    public int PollBatchSize { get; init; } = 1;
    public TimeSpan PollWait { get; init; } = TimeSpan.FromSeconds(25);
    public TimeSpan RequestTimeout { get; init; } = TimeSpan.FromSeconds(60);
    public TimeSpan RenewLead { get; init; } = TimeSpan.FromSeconds(10);
    public TimeSpan RetryBase { get; init; } = TimeSpan.FromMilliseconds(500);
    public TimeSpan RetryMax { get; init; } = TimeSpan.FromSeconds(10);
    public int? RetryMaxAttempts { get; init; }
    public TimeSpan HeartbeatInterval { get; init; } = TimeSpan.Zero;
    public IReadOnlyDictionary<string, object?>? HeartbeatMetadata { get; init; }
    public bool ParsePayloadJson { get; init; }
    public bool RegisterJobs { get; init; } = true;
    public string? OutboxPath { get; init; }
    public int OutboxMaxEntries { get; init; } = 500;
    public long OutboxMaxBytes { get; init; } = 1_000_000;
    public ILogger? Logger { get; init; }

    public static RunnerConfig FromEnvironment(IDictionary<string, string?>? env = null)
    {
        env ??= System.Environment.GetEnvironmentVariables()
            .Cast<System.Collections.DictionaryEntry>()
            .ToDictionary(entry => entry.Key.ToString() ?? string.Empty, entry => entry.Value?.ToString());

        string Require(string key)
        {
            var value = GetOptional(env, key);
            if (string.IsNullOrWhiteSpace(value))
            {
                throw new InvalidOperationException($"{key} is required");
            }
            return value.Trim();
        }

        var baseUrl = Require("CRONIQ_API_BASEURL");
        var tenantId = Require("CRONIQ_TENANT_ID");
        var environment = Require("CRONIQ_ENVIRONMENT");
        var runnerId = Require("CRONIQ_RUNNER_ID");
        var runnerInstanceId = GetOptional(env, "CRONIQ_RUNNER_INSTANCE_ID");
        if (string.IsNullOrWhiteSpace(runnerInstanceId))
        {
            runnerInstanceId = Guid.NewGuid().ToString("N");
        }

        var apiKey = GetOptional(env, "CRONIQ_API_KEY");
        var bearerToken = GetOptional(env, "CRONIQ_BEARER_TOKEN");
        var hasApiKey = !string.IsNullOrWhiteSpace(apiKey);
        var hasBearer = !string.IsNullOrWhiteSpace(bearerToken);
        if (hasApiKey == hasBearer)
        {
            throw new InvalidOperationException("Set exactly one of CRONIQ_API_KEY or CRONIQ_BEARER_TOKEN");
        }

        var transportModeValue = GetOptional(env, "CRONIQ_TRANSPORT_MODE")?.Trim().ToLowerInvariant() ?? "auto";
        var transportMode = transportModeValue switch
        {
            "auto" => TransportMode.Auto,
            "grpc" => TransportMode.Grpc,
            "polling" => TransportMode.Polling,
            _ => throw new InvalidOperationException("CRONIQ_TRANSPORT_MODE must be auto, grpc, or polling")
        };

        var registerJobs = ParseOptionalBool(env, "CRONIQ_RUNNER_REGISTER_JOBS") ?? true;

        return new RunnerConfig
        {
            BaseUrl = baseUrl,
            GrpcBaseUrl = GetOptional(env, "CRONIQ_GRPC_BASEURL"),
            TenantId = tenantId,
            Environment = environment,
            RunnerId = runnerId,
            RunnerInstanceId = runnerInstanceId,
            ApiKey = hasApiKey ? apiKey : null,
            BearerToken = hasBearer ? bearerToken : null,
            TransportMode = transportMode,
            AllowTestExecutions = ParseBool(env, "CRONIQ_ALLOW_TEST_EXECUTIONS"),
            MaxInflight = ParseInt(env, "CRONIQ_MAX_INFLIGHT") ?? 1,
            Capabilities = ParseList(env, "CRONIQ_CAPABILITIES"),
            PollBatchSize = ParseInt(env, "CRONIQ_POLL_BATCH_SIZE") ?? 1,
            PollWait = TimeSpan.FromMilliseconds(ParseInt(env, "CRONIQ_POLL_WAIT_MS") ?? 25000),
            RequestTimeout = TimeSpan.FromMilliseconds(ParseInt(env, "CRONIQ_REQUEST_TIMEOUT_MS") ?? 60000),
            RenewLead = TimeSpan.FromMilliseconds(ParseInt(env, "CRONIQ_RENEW_LEAD_MS") ?? 10000),
            RetryBase = TimeSpan.FromMilliseconds(ParseInt(env, "CRONIQ_RETRY_BASE_MS") ?? 500),
            RetryMax = TimeSpan.FromMilliseconds(ParseInt(env, "CRONIQ_RETRY_MAX_MS") ?? 10000),
            RetryMaxAttempts = ParseInt(env, "CRONIQ_RETRY_MAX_ATTEMPTS"),
            OutboxPath = GetOptional(env, "CRONIQ_OUTBOX_PATH"),
            RegisterJobs = registerJobs
        };
    }

    private static string? GetOptional(IDictionary<string, string?> env, string key)
        => env.TryGetValue(key, out var value) ? value?.Trim() : null;

    private static bool ParseBool(IDictionary<string, string?> env, string key)
    {
        var raw = GetOptional(env, key);
        if (string.IsNullOrWhiteSpace(raw))
        {
            return false;
        }

        return raw.Trim().ToLowerInvariant() switch
        {
            "true" or "1" => true,
            "false" or "0" => false,
            _ => throw new InvalidOperationException($"Invalid boolean value for {key}: {raw}")
        };
    }

    private static bool? ParseOptionalBool(IDictionary<string, string?> env, string key)
    {
        var raw = GetOptional(env, key);
        if (string.IsNullOrWhiteSpace(raw))
        {
            return null;
        }

        return raw.Trim().ToLowerInvariant() switch
        {
            "true" or "1" => true,
            "false" or "0" => false,
            _ => throw new InvalidOperationException($"Invalid boolean value for {key}: {raw}")
        };
    }

    private static int? ParseInt(IDictionary<string, string?> env, string key)
    {
        var raw = GetOptional(env, key);
        if (string.IsNullOrWhiteSpace(raw))
        {
            return null;
        }

        if (!int.TryParse(raw, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value))
        {
            throw new InvalidOperationException($"Invalid integer value for {key}: {raw}");
        }

        return value;
    }

    private static string[]? ParseList(IDictionary<string, string?> env, string key)
    {
        var raw = GetOptional(env, key);
        if (string.IsNullOrWhiteSpace(raw))
        {
            return null;
        }

        return raw.Split(',')
            .Select(value => value.Trim())
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .ToArray();
    }
}
