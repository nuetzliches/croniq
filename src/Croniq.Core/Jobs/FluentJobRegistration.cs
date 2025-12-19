using System;
using Croniq.Sdk;

namespace Croniq.Core.Jobs;

public sealed class FluentJobRegistration : JobRegistration
{
    public FluentJobRegistration(Type jobType, CroniqJobAttribute attribute) : base(jobType)
    {
        Attribute = attribute ?? throw new ArgumentNullException(nameof(attribute));
    }

    public CroniqJobAttribute Attribute { get; }
}
