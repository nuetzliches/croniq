using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using Croniq.Core.Options;
using Croniq.Sdk;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Jobs;

public sealed class JobRegistry : IJobRegistry
{
    private readonly Dictionary<string, JobDescriptor> _descriptors;

    public JobRegistry(IOptions<CroniqOptions> options, IEnumerable<JobRegistration> registrations)
    {
        if (options is null) throw new ArgumentNullException(nameof(options));
        _descriptors = new Dictionary<string, JobDescriptor>(StringComparer.OrdinalIgnoreCase);
        foreach (var registration in registrations ?? Enumerable.Empty<JobRegistration>())
        {
            if (registration is FluentJobRegistration fluent)
            {
                Add(fluent.JobType, options.Value, fluent.Attribute);
                continue;
            }

            Add(registration.JobType, options.Value, null);
        }
    }

    public IReadOnlyCollection<JobDescriptor> Descriptors => _descriptors.Values.ToArray();

    public bool TryGet(JobKey jobKey, out JobDescriptor descriptor)
    {
        return _descriptors.TryGetValue(jobKey.Value, out descriptor!);
    }

    private void Add(Type jobType, CroniqOptions options, CroniqJobAttribute? attributeOverride)
    {
        var attribute = attributeOverride ?? jobType.GetCustomAttribute<CroniqJobAttribute>();
        if (attribute is null)
        {
            throw new InvalidOperationException($"Type {jobType.FullName} is missing CroniqJobAttribute.");
        }

        var jobKey = JobKey.Create(options.TenantId, options.EnvironmentTag, attribute.NamespaceSegment, attribute.JobName, attribute.Variant);

        if (_descriptors.ContainsKey(jobKey.Value))
        {
            throw new InvalidOperationException($"JobKey {jobKey} is already registered.");
        }

        _descriptors[jobKey.Value] = new JobDescriptor(jobType, attribute, jobKey);
    }
}
