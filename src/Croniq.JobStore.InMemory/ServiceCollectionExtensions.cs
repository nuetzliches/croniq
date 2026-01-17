using System;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq.JobStore.InMemory;

/// <summary>
/// DI helpers for wiring the in-memory job store.
/// </summary>
public static class ServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqInMemoryJobStore(this IServiceCollection services, Action<InMemoryJobStoreOptions>? configure = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));

        services.AddOptions<InMemoryJobStoreOptions>();
        if (configure is not null)
        {
            services.Configure(configure);
        }

        services.TryAddSingleton<IJobPersistenceProvider, InMemoryJobStore>();
        services.TryAddSingleton<IJobStore>(sp => (IJobStore)sp.GetRequiredService<IJobPersistenceProvider>());
        services.TryAddSingleton<IJobDeadLetterStore>(sp => (IJobDeadLetterStore)sp.GetRequiredService<IJobPersistenceProvider>());
        services.TryAddSingleton<ICalendarStore>(sp => (ICalendarStore)sp.GetRequiredService<IJobPersistenceProvider>());

        services.AddOptions<WorkerStoreOptions>();
        services.AddSingleton<IWorkerStore, InMemoryWorkerStore>();
        services.AddOptions<RunnerStoreOptions>();
        services.AddSingleton<IRunnerStore, InMemoryRunnerStore>();

        return services;
    }
}
