using System;

namespace Croniq.Core.Jobs;

public sealed class JobRegistration
{
    public JobRegistration(Type jobType)
    {
        JobType = jobType ?? throw new ArgumentNullException(nameof(jobType));
    }

    public Type JobType { get; }
}
