using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Sdk;

namespace Croniq.Core.Jobs;

public sealed class DelegatingJob : IJob
{
    private readonly IJobHandlerRegistry _handlers;
    private readonly IServiceProvider _serviceProvider;

    public DelegatingJob(IJobHandlerRegistry handlers, IServiceProvider serviceProvider)
    {
        _handlers = handlers ?? throw new ArgumentNullException(nameof(handlers));
        _serviceProvider = serviceProvider ?? throw new ArgumentNullException(nameof(serviceProvider));
    }

    public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
    {
        if (context is null) throw new ArgumentNullException(nameof(context));

        if (!_handlers.TryGet(context.JobKey, out var handler))
        {
            throw new InvalidOperationException($"No handler registered for job key '{context.JobKey}'.");
        }

        return handler(_serviceProvider, context, cancellationToken);
    }
}
