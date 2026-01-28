using System;
using System.Collections.Generic;

namespace Croniq.Runner;

public sealed class CroniqRunnerOptions
{
    public RunnerConfig? Config { get; set; }
    public TimeSpan DrainTimeout { get; set; } = TimeSpan.FromSeconds(30);

    internal Dictionary<string, HandlerRegistration> Handlers { get; } = new(StringComparer.OrdinalIgnoreCase);

    public void OnExecute(string jobKey, RunnerExecuteHandler handler)
        => OnExecute(jobKey, handler, registration: null);

    public void OnExecute(string jobKey, RunnerExecuteHandler handler, RunnerJobRegistration? registration)
    {
        if (string.IsNullOrWhiteSpace(jobKey))
        {
            throw new ArgumentException("jobKey is required.", nameof(jobKey));
        }
        if (handler is null)
        {
            throw new ArgumentNullException(nameof(handler));
        }

        Handlers[jobKey.Trim()] = new HandlerRegistration(handler, registration);
    }
}

internal sealed record HandlerRegistration(RunnerExecuteHandler Handler, RunnerJobRegistration? Registration);
