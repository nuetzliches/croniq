using System;
using System.Diagnostics;
using System.Reflection;
using Croniq.Core.Execution;
using Croniq.Core.Hosting;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.Sdk;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Logging;

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

        services.Configure<MisfirePolicyOptions>(_ => { });
        services.Configure<ExecutionPolicyOptions>(_ => { });
        services.Configure<PolicyOverrideOptions>(_ => { });
        services.TryAddSingleton<IMisfirePolicy, DefaultMisfirePolicy>();
        services.TryAddSingleton<IPolicyResolver, PolicyResolver>();
        services.TryAddSingleton<IExecutionPolicyPipelineProvider, ExecutionPolicyPipelineProvider>();
        services.TryAddSingleton<IQuotaGuard, InMemoryQuotaGuard>();
        services.TryAddSingleton<ActivitySource>(_ => new ActivitySource("Croniq.Core"));
        services.TryAddSingleton<IJobRegistry, JobRegistry>();
        services.TryAddSingleton<IJobExecutionPipeline, DefaultJobExecutionPipeline>();
        services.TryAddSingleton<IJobTrigger, DefaultJobTrigger>();
        services.TryAddSingleton<IExecutionLogStore, NoOpExecutionLogStore>();
        services.TryAddSingleton<IExecutionLogExporter, LoggerExecutionLogExporter>();
        services.TryAddSingleton<IExecutionLogReader, NoOpExecutionLogReader>();
        services.TryAddSingleton<IExecutionHistoryReader, NoOpExecutionHistoryReader>();
        services.TryAddSingleton<TriggerWorker>();

        return services;
    }

    public static IServiceCollection AddCroniqFileExecutionLogStore(this IServiceCollection services, Action<FileExecutionLogStoreOptions>? configure = null)
    {
        var options = new FileExecutionLogStoreOptions();
        configure?.Invoke(options);
        services.RemoveAll(typeof(IExecutionLogStore));
        services.RemoveAll(typeof(IExecutionLogReader));
        services.RemoveAll(typeof(IExecutionHistoryReader));
        services.AddSingleton(options);
        services.AddSingleton<IExecutionLogStore, FileExecutionLogStore>();
        services.AddSingleton<IExecutionLogReader, FileExecutionLogReader>();
        services.AddSingleton<IExecutionHistoryReader, FileExecutionHistoryReader>();
        return services;
    }

    public static ILoggingBuilder AddCroniqExecutionLogSink(this ILoggingBuilder builder, Action<ExecutionLogSinkOptions>? configure = null)
    {
        if (configure is not null)
        {
            builder.Services.Configure(configure);
        }
        builder.Services.AddSingleton<ILoggerProvider, ExecutionLogSinkProvider>();
        return builder;
    }

    public static IServiceCollection AddCroniqWorkerHost(this IServiceCollection services, Action<WorkerHostOptions>? configure = null)
    {
        services.AddOptions<WorkerHostOptions>();
        if (configure is not null)
        {
            services.Configure(configure);
        }
        services.AddHostedService<CroniqWorkerHostedService>();
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
        services.TryAddEnumerable(ServiceDescriptor.Singleton<JobRegistration, JobRegistration<TJob>>());
        return services;
    }
}
