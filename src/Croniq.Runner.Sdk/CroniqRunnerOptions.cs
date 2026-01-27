using System;
using System.Collections.Generic;

namespace Croniq.Runner;

public sealed class CroniqRunnerOptions
{
    public RunnerConfig? Config { get; set; }
    public TimeSpan DrainTimeout { get; set; } = TimeSpan.FromSeconds(30);

    internal Dictionary<string, RunnerExecuteHandler> Handlers { get; } = new(StringComparer.OrdinalIgnoreCase);

    public void OnExecute(string jobKey, RunnerExecuteHandler handler)
    {
        if (string.IsNullOrWhiteSpace(jobKey))
        {
            throw new ArgumentException("jobKey is required.", nameof(jobKey));
        }
        if (handler is null)
        {
            throw new ArgumentNullException(nameof(handler));
        }

        Handlers[jobKey.Trim()] = handler;
    }
}
