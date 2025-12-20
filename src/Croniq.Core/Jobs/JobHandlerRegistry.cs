using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Options;
using Croniq.Sdk;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Jobs;

public interface IJobHandlerRegistry
{
    bool TryGet(string jobKey, out Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler);
}

public sealed class JobHandlerRegistration
{
    public JobHandlerRegistration(CroniqJobAttribute attribute, Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler)
    {
        Attribute = attribute ?? throw new ArgumentNullException(nameof(attribute));
        Handler = handler ?? throw new ArgumentNullException(nameof(handler));
    }

    public CroniqJobAttribute Attribute { get; }

    public Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> Handler { get; }
}

public sealed class JobHandlerRegistry : IJobHandlerRegistry
{
    private readonly Dictionary<string, Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task>> _handlers;

    public JobHandlerRegistry(IOptions<CroniqOptions> options, IEnumerable<JobHandlerRegistration> registrations)
    {
        if (options is null) throw new ArgumentNullException(nameof(options));
        _handlers = new Dictionary<string, Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task>>(StringComparer.OrdinalIgnoreCase);

        foreach (var registration in registrations ?? Enumerable.Empty<JobHandlerRegistration>())
        {
            var attribute = registration.Attribute;
            var jobKey = JobKey.Create(
                options.Value.TenantId,
                options.Value.EnvironmentTag,
                attribute.NamespaceSegment,
                attribute.JobName,
                attribute.Variant);

            if (_handlers.ContainsKey(jobKey.Value))
            {
                throw new InvalidOperationException($"JobKey {jobKey} is already registered.");
            }

            _handlers[jobKey.Value] = registration.Handler;
        }
    }

    public bool TryGet(string jobKey, out Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler)
    {
        if (string.IsNullOrWhiteSpace(jobKey))
        {
            handler = null!;
            return false;
        }

        return _handlers.TryGetValue(jobKey, out handler!);
    }
}
