using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;

namespace Croniq.Core.Execution;

public sealed class JobExecutionRequest
{
    private static readonly IReadOnlyDictionary<string, string> EmptyMetadata =
        new ReadOnlyDictionary<string, string>(new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase));

    public JobExecutionRequest(
        string executionId,
        JobKey jobKey,
        JobDescriptor descriptor,
        ExecutionPolicyOptions? executionOptions = null,
        IReadOnlyDictionary<string, string>? metadata = null,
        ActivitySource? activitySource = null)
    {
        ExecutionId = string.IsNullOrWhiteSpace(executionId)
            ? throw new ArgumentException("ExecutionId must be provided", nameof(executionId))
            : executionId;
        JobKey = jobKey;
        Descriptor = descriptor;
        ExecutionOptions = executionOptions;
        Metadata = metadata ?? EmptyMetadata;
        ActivitySource = activitySource;
    }

    public string ExecutionId { get; }

    public JobKey JobKey { get; }

    public JobDescriptor Descriptor { get; }

    public ExecutionPolicyOptions? ExecutionOptions { get; }

    public IReadOnlyDictionary<string, string> Metadata { get; }

    public ActivitySource? ActivitySource { get; }
}
