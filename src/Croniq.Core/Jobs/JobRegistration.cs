using System;
using Croniq.Sdk;

namespace Croniq.Core.Jobs;

public class JobRegistration
{
    public JobRegistration(Type jobType)
    {
        JobType = jobType ?? throw new ArgumentNullException(nameof(jobType));
    }

    public Type JobType { get; }
}

public sealed class JobRegistration<TJob> : JobRegistration where TJob : class, IJob
{
    public JobRegistration() : base(typeof(TJob))
    {
    }
}
