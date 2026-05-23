using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Map of <c>job_key</c> → handler-resolution-delegate. Populated by the
/// <c>AddCroniqJob</c> family of extension methods. Holds either a typed
/// <see cref="ICroniqJobHandler"/> (resolved per execution from a fresh
/// DI scope) or a free-form delegate.
/// </summary>
internal sealed class CroniqHandlerRegistry
{
    private readonly Dictionary<string, HandlerEntry> _handlers = new(StringComparer.Ordinal);
    private HandlerEntry? _defaultHandler;
    private readonly List<JobSchedule> _selfRegisterSchedules = [];

    public void RegisterDelegate(string jobKey, Func<CroniqExecutionContext, CancellationToken, Task> handler)
    {
        _handlers[jobKey] = new HandlerEntry.Delegate(handler);
    }

    public void RegisterInterface(string jobKey, Type handlerType)
    {
        _handlers[jobKey] = new HandlerEntry.Interface(handlerType);
    }

    public void SetDefaultDelegate(Func<CroniqExecutionContext, CancellationToken, Task> handler)
    {
        _defaultHandler = new HandlerEntry.Delegate(handler);
    }

    public void SetDefaultInterface(Type handlerType)
    {
        _defaultHandler = new HandlerEntry.Interface(handlerType);
    }

    public bool TryGet(string jobKey, out HandlerEntry entry)
    {
        if (_handlers.TryGetValue(jobKey, out var match))
        {
            entry = match;
            return true;
        }
        if (_defaultHandler is not null)
        {
            entry = _defaultHandler;
            return true;
        }
        entry = default!;
        return false;
    }

    public void AddSelfRegisterSchedule(string jobKey, string schedule, string? timeout, string? description)
    {
        _selfRegisterSchedules.Add(new JobSchedule(jobKey, schedule, timeout, description));
    }

    public IReadOnlyList<JobSchedule> SelfRegisterSchedules => _selfRegisterSchedules;

    public IReadOnlyCollection<string> RegisteredJobKeys => _handlers.Keys;

    public bool HasDefaultHandler => _defaultHandler is not null;

    internal abstract record HandlerEntry
    {
        public sealed record Delegate(Func<CroniqExecutionContext, CancellationToken, Task> Handler) : HandlerEntry;
        public sealed record Interface(Type HandlerType) : HandlerEntry;

        public async Task InvokeAsync(IServiceProvider services, CroniqExecutionContext context, CancellationToken cancellationToken)
        {
            switch (this)
            {
                case Delegate d:
                    await d.Handler(context, cancellationToken).ConfigureAwait(false);
                    return;
                case Interface i:
                    await using (var scope = services.CreateAsyncScope())
                    {
                        var handler = (ICroniqJobHandler)scope.ServiceProvider.GetRequiredService(i.HandlerType);
                        await handler.HandleAsync(context, cancellationToken).ConfigureAwait(false);
                    }
                    return;
                default:
                    throw new InvalidOperationException("Unknown handler entry");
            }
        }
    }

    internal sealed record JobSchedule(string JobKey, string Schedule, string? Timeout, string? Description);
}
