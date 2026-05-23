using Croniq.Runner.Sdk;
using Croniq.Runner.Sdk.DependencyInjection;
using Croniq.Runner.Sdk.Logging;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Translates a YAML handler <c>behavior</c> into a Croniq handler delegate.
/// Every conformance case binding must implement the same sentinels:
/// <c>noop</c>, <c>throw</c>, <c>sleep</c>, <c>log</c>, <c>stream_logs</c>.
/// </summary>
internal static class HandlerSentinels
{
    public static ICroniqRunnerBuilder ApplyTo(ICroniqRunnerBuilder builder, IList<HandlerSpec> handlers)
    {
        foreach (var spec in handlers)
        {
            var snapshot = spec; // capture
            Func<CroniqExecutionContext, CancellationToken, Task> handler = snapshot.Behavior switch
            {
                "noop" => static (_, _) => Task.CompletedTask,
                "throw" => (_, _) => throw new InvalidOperationException(snapshot.ErrorMessage ?? "thrown by conformance handler"),
                "sleep" => async (_, ct) => await Task.Delay(snapshot.DurationMs ?? 0, ct).ConfigureAwait(false),
                "log" => (ctx, _) =>
                {
                    var level = ParseLevel(snapshot.Level);
                    var count = snapshot.Count ?? 1;
                    for (var i = 0; i < count; i++)
                    {
#pragma warning disable CA2254 // template not constant — conformance shim only
                        ctx.Logger.Log(level, snapshot.Message ?? "");
#pragma warning restore CA2254
                    }
                    return Task.CompletedTask;
                }
                ,
                "stream_logs" => async (ctx, ct) =>
                {
                    var count = snapshot.Count ?? 1;
                    var interval = snapshot.IntervalMs ?? 0;
                    var level = ParseLevel(snapshot.Level);
                    var writer = ctx.LogWriter;
                    for (var i = 0; i < count; i++)
                    {
                        await writer.WriteAsync(level, $"line {i + 1}", cancellationToken: ct).ConfigureAwait(false);
                        if (interval > 0 && i + 1 < count)
                        {
                            await Task.Delay(interval, ct).ConfigureAwait(false);
                        }
                    }
                }
                ,
                _ => throw new NotSupportedException($"unknown handler behavior '{snapshot.Behavior}'"),
            };

            if (snapshot.IsDefault)
            {
                builder.AddCroniqDefaultHandler(handler);
            }
            else if (!string.IsNullOrEmpty(snapshot.Schedule))
            {
                builder.AddCroniqJob(snapshot.JobKey, snapshot.Schedule, handler);
            }
            else
            {
                builder.AddCroniqJob(snapshot.JobKey, handler);
            }
        }
        return builder;
    }

    private static LogLevel ParseLevel(string? level) => (level ?? "info").ToLowerInvariant() switch
    {
        "trace" => LogLevel.Trace,
        "debug" => LogLevel.Debug,
        "info" => LogLevel.Information,
        "warn" => LogLevel.Warning,
        "error" => LogLevel.Error,
        _ => LogLevel.Information,
    };
}
