using System;
using System.Diagnostics;
using System.Reflection;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Sdk;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq.Core;

public static class ServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqCore(this IServiceCollection services, Action<CroniqOptions>? configure = null)
    {
        services.AddOptions<CroniqOptions>();
        if (configure is not null)
        {
            services.Configure(configure);
        }

        services.TryAddSingleton<ActivitySource>(_ => new ActivitySource("Croniq.Core"));
        services.TryAddSingleton<IJobRegistry, JobRegistry>();
        services.TryAddSingleton<IJobExecutionPipeline, DefaultJobExecutionPipeline>();
        services.TryAddSingleton<TriggerWorker>();

        return services;
    }

    public static IServiceCollection AddCroniqJob<TJob>(this IServiceCollection services)
        where TJob : class, IJob
    {
        var attribute = typeof(TJob).GetCustomAttribute<CroniqJobAttribute>();
        if (attribute is null)
        {
            throw new InvalidOperationException($"Type {typeof(TJob).FullName} is missing CroniqJobAttribute.");
        }

        services.AddTransient<TJob>();
        services.TryAddEnumerable(ServiceDescriptor.Singleton(new JobRegistration(typeof(TJob))));
        return services;
    }
}
