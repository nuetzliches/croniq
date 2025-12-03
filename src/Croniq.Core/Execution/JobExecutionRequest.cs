using System.Collections.Generic;
using System.Diagnostics;
using Croniq.Core.Jobs;

namespace Croniq.Core.Execution;

public sealed class JobExecutionRequest
{
    public JobExecutionRequest(JobKey jobKey, JobDescriptor descriptor, IReadOnlyDictionary<string, string>? metadata = null, ActivitySource? activitySource = null)
    {
        JobKey = jobKey;
        Descriptor = descriptor;
        Metadata = metadata;
        ActivitySource = activitySource;
    }

    public JobKey JobKey { get; }

    public JobDescriptor Descriptor { get; }

    public IReadOnlyDictionary<string, string>? Metadata { get; }

    public ActivitySource? ActivitySource { get; }
}
