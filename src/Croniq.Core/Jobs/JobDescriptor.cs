using System;
using Croniq.Sdk;

namespace Croniq.Core.Jobs;

public sealed class JobDescriptor
{
    public JobDescriptor(Type jobType, CroniqJobAttribute attribute, JobKey jobKey)
    {
        JobType = jobType ?? throw new ArgumentNullException(nameof(jobType));
        Attribute = attribute ?? throw new ArgumentNullException(nameof(attribute));
        JobKey = jobKey;
    }

    public Type JobType { get; }

    public CroniqJobAttribute Attribute { get; }

    public JobKey JobKey { get; }

    public string QualifiedName => Attribute.Variant is null
        ? $"{Attribute.NamespaceSegment}:{Attribute.JobName}"
        : $"{Attribute.NamespaceSegment}:{Attribute.JobName}:{Attribute.Variant}";
}
