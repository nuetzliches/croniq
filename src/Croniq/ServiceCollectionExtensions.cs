using System;
using Croniq.Core;
using Croniq.Core.Hosting;
using Croniq.Core.Jobs;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.JobStore.InMemory;
using Croniq.Providers.Default;
using Croniq.Sdk;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq;

public static class ServiceCollectionExtensions
{
    private const string CoreSectionPath = "Croniq:Core";
    private const string WorkerHostSectionPath = "Croniq:WorkerHost";
    private const string InMemoryJobStoreSectionPath = "Croniq:JobStore:InMemory";
    private const string SeedingSectionPath = "Croniq:Seeding";
    private const string MisfirePolicySectionPath = "Croniq:Policies:Misfire";
    private const string ExecutionPolicySectionPath = "Croniq:Policies:Execution";
    private const string PolicyOverridesSectionPath = "Croniq:Policies:Overrides";

    public static IServiceCollection AddCroniq(
        this IServiceCollection services,
        Action<CroniqWorkerOptions>? configure = null)
    {
        return AddCroniqWorker(services, configure);
    }

    public static IServiceCollection AddCroniq(
        this IServiceCollection services,
        IConfiguration configuration,
        Action<CroniqWorkerOptions>? configure = null)
    {
        return AddCroniqWorker(services, configuration, configure);
    }

    public static IServiceCollection AddCroniqWorker(
        this IServiceCollection services,
        Action<CroniqWorkerOptions>? configure = null)
    {
        return AddCroniqWorker(services, new ConfigurationBuilder().Build(), configure);
    }

    public static IServiceCollection AddCroniqWorker(
        this IServiceCollection services,
        IConfiguration configuration,
        Action<CroniqWorkerOptions>? configure = null)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configuration);

        services.Configure<CroniqOptions>(configuration.GetSection(CoreSectionPath));
        services.Configure<WorkerHostOptions>(configuration.GetSection(WorkerHostSectionPath));
        services.Configure<InMemoryJobStoreOptions>(configuration.GetSection(InMemoryJobStoreSectionPath));
        services.Configure<CroniqSeedingOptions>(configuration.GetSection(SeedingSectionPath));
        services.Configure<MisfirePolicyOptions>(configuration.GetSection(MisfirePolicySectionPath));
        services.Configure<ExecutionPolicyOptions>(configuration.GetSection(ExecutionPolicySectionPath));
        services.Configure<PolicyOverrideOptions>(configuration.GetSection(PolicyOverridesSectionPath));

        if (configure is not null)
        {
            var options = new CroniqWorkerOptions();
            configure(options);

            if (!string.IsNullOrWhiteSpace(options.TenantId)
                || !string.IsNullOrWhiteSpace(options.EnvironmentTag)
                || !string.IsNullOrWhiteSpace(options.InstanceId))
            {
                services.PostConfigure<CroniqOptions>(core =>
                {
                    if (!string.IsNullOrWhiteSpace(options.TenantId))
                    {
                        core.TenantId = options.TenantId!;
                    }

                    if (!string.IsNullOrWhiteSpace(options.EnvironmentTag))
                    {
                        core.EnvironmentTag = options.EnvironmentTag!;
                    }

                    if (!string.IsNullOrWhiteSpace(options.InstanceId))
                    {
                        core.InstanceId = options.InstanceId!;
                    }
                });
            }

            if (options.BatchSize.HasValue
                || options.IdleDelay.HasValue
                || options.BusyDelay.HasValue
                || options.ErrorDelay.HasValue)
            {
                services.PostConfigure<WorkerHostOptions>(worker =>
                {
                    if (options.BatchSize.HasValue)
                    {
                        worker.BatchSize = options.BatchSize.Value;
                    }

                    if (options.IdleDelay.HasValue)
                    {
                        worker.IdleDelay = options.IdleDelay.Value;
                    }

                    if (options.BusyDelay.HasValue)
                    {
                        worker.BusyDelay = options.BusyDelay.Value;
                    }

                    if (options.ErrorDelay.HasValue)
                    {
                        worker.ErrorDelay = options.ErrorDelay.Value;
                    }
                });
            }

            if (options.InMemoryLeaseDurationSeconds.HasValue)
            {
                services.PostConfigure<InMemoryJobStoreOptions>(store =>
                {
                    store.LeaseDurationSeconds = options.InMemoryLeaseDurationSeconds.Value;
                });
            }

            options.ConfigureServices?.Invoke(services);
        }

        services.AddCroniqDefaultProviders();
        services.AddCroniqCore();
        services.AddCroniqInMemoryJobStore();
        services.AddHostedService<CroniqTriggerSeedingHostedService>();
        services.AddHostedService<CroniqTriggerSummaryHostedService>();
        services.AddCroniqWorkerHost();

        return services;
    }

    public static IServiceCollection AddCroniqJob<TJob>(this IServiceCollection services)
        where TJob : class, IJob
    {
        return Core.ServiceCollectionExtensions.AddCroniqJob<TJob>(services);
    }

    public static CroniqJobBuilder AddCroniqJob(
        this IServiceCollection services,
        string namespaceSegment,
        string jobName,
        Func<IJobExecutionContext, CancellationToken, Task> handler,
        string? variant = null)
    {
        if (handler is null) throw new ArgumentNullException(nameof(handler));
        return AddCroniqJob(services, namespaceSegment, jobName, (sp, ctx, token) => handler(ctx, token), variant);
    }

    public static CroniqJobBuilder AddCroniqJob(
        this IServiceCollection services,
        string namespaceSegment,
        string jobName,
        Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler,
        string? variant = null)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(handler);

        var attribute = new CroniqJobAttribute(namespaceSegment, jobName, variant);

        services.TryAddSingleton<IJobHandlerRegistry, JobHandlerRegistry>();
        services.TryAddTransient<DelegatingJob>();

        services.AddSingleton<JobRegistration>(new FluentJobRegistration(typeof(DelegatingJob), attribute));
        services.AddSingleton(new JobHandlerRegistration(attribute, handler));

        return new CroniqJobBuilder(services, attribute);
    }
}
