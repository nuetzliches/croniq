using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using Croniq.Sdk;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;

namespace Croniq.Core.Execution;

public sealed class JobExecutionContext : IJobExecutionContext
{
    private static readonly IReadOnlyDictionary<string, string> EmptyMetadata =
        new ReadOnlyDictionary<string, string>(new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase));

    public JobExecutionContext(string jobKey, IReadOnlyDictionary<string, string>? metadata, ILogger logger, ActivitySource activitySource)
    {
        JobKey = jobKey ?? throw new ArgumentNullException(nameof(jobKey));
        Metadata = metadata ?? EmptyMetadata;
        Logger = logger ?? NullLogger.Instance;
        ActivitySource = activitySource ?? new ActivitySource("Croniq.Core.Job");
    }

    public string JobKey { get; }

    public IReadOnlyDictionary<string, string> Metadata { get; }

    public ILogger Logger { get; }

    public ActivitySource ActivitySource { get; }
}
