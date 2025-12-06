using System.Collections.Generic;
using System.Diagnostics;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;

namespace Croniq.Core.Execution;

public sealed class JobExecutionRequest
{
    public JobExecutionRequest(JobKey jobKey, JobDescriptor descriptor, ExecutionPolicyOptions? executionOptions = null, IReadOnlyDictionary<string, string>? metadata = null, ActivitySource? activitySource = null)
    {
        JobKey = jobKey;
        Descriptor = descriptor;
        ExecutionOptions = executionOptions;
        Metadata = metadata;
        ActivitySource = activitySource;
    }

    public JobKey JobKey { get; }

    public JobDescriptor Descriptor { get; }

    public ExecutionPolicyOptions? ExecutionOptions { get; }

    public IReadOnlyDictionary<string, string>? Metadata { get; }

    public ActivitySource? ActivitySource { get; }
}
