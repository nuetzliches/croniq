using System;
using System.Diagnostics;
using System.Reflection;
using Croniq.Core.Execution;
using Croniq.Core.Health;
using Croniq.Core.Hosting;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Core.Scheduling;
using Croniq.Sdk;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Diagnostics.HealthChecks;
using Microsoft.Extensions.Logging;

namespace Croniq.Core;

public static class ServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqCore(this IServiceCollection services, Action<CroniqOptions>? configure = null)
    {
        services.AddOptions<CroniqOptions>()
            .Validate(ValidateCroniqOptions, "Croniq:Core must set a valid tenant configuration, EnvironmentTag, and InstanceId.")
            .ValidateOnStart();
        services.AddOptions<CroniqStartupOptions>()
            .Validate(ValidateStartupOptions, "Croniq:Startup:Mode must be Run or Validate.")
            .ValidateOnStart();
        services.AddOptions<CroniqSeedingOptions>()
            .Validate(ValidateSeedingOptions, "Croniq:Seeding:Mode must be Off, CreateIfMissing, or ForceUpdate.")
            .ValidateOnStart();
        services.AddOptions<CroniqJobRegistrySyncOptions>()
            .Validate(ValidateJobRegistrySyncOptions, "Croniq:JobRegistrySync:Mode must be Off, CreateIfMissing, or ForceUpdate.")
            .ValidateOnStart();
        services.AddOptions<CroniqRetentionOptions>()
            .Validate(ValidateRetentionOptions, "Croniq:Retention must define a valid schedule and retention settings.")
            .ValidateOnStart();
        services.AddOptions<WorkerHostOptions>();
        services.AddOptions<RunnerStoreOptions>();
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
        services.TryAddSingleton<IRunnerStore, NoOpRunnerStore>();
        services.TryAddSingleton<IWorkItemStore, NoOpWorkItemStore>();
        services.TryAddSingleton<TriggerWorker>();

        // Explicit env var overrides (do not require .NET configuration key mapping)
        services.PostConfigure<CroniqOptions>(core =>
        {
            var rawMode = Environment.GetEnvironmentVariable("CRONIQ_CORE_TENANT_MODE");
            if (!string.IsNullOrWhiteSpace(rawMode)
                && Enum.TryParse<TenantMode>(rawMode.Trim(), ignoreCase: true, out var parsed))
            {
                core.TenantMode = parsed;
            }

            var rawTenantId = Environment.GetEnvironmentVariable("CRONIQ_CORE_TENANT_ID");
            if (!string.IsNullOrWhiteSpace(rawTenantId))
            {
                core.TenantId = rawTenantId.Trim();
            }
        });

        return services;
    }

    private static bool ValidateRetentionOptions(CroniqRetentionOptions options)
    {
        if (options is null)
        {
            return false;
        }

        if (options.RefreshTokensRetentionDays < -1)
        {
            return false;
        }

        if (options.JobDeadLettersExpiryOffsetDays < -1)
        {
            return false;
        }

        if (options.WebhookDeadLettersExpiryOffsetDays < -1)
        {
            return false;
        }

        if (options.WebhookEndpointEventsRetentionDays < -1)
        {
            return false;
        }

        if (options.WebhookSecretHistoryExpiryOffsetDays < -1)
        {
            return false;
        }

        if (!options.Enabled)
        {
            return true;
        }

        if (string.IsNullOrWhiteSpace(options.ScheduleCron))
        {
            return false;
        }

        try
        {
            _ = new CronSchedule(options.ScheduleCron.Trim(), TimeZoneUtil.ResolveTimeZone(options.TimeZoneId));
        }
        catch
        {
            return false;
        }

        return true;
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
        services.AddOptions<WorkerHostOptions>()
            .Validate(ValidateWorkerHostOptions, "Croniq:WorkerHost must set BatchSize > 0 and non-negative delays.")
            .ValidateOnStart();
        if (configure is not null)
        {
            services.Configure(configure);
        }
        services.AddHostedService<CroniqWorkerHostedService>();
        return services;
    }

    public static IServiceCollection AddCroniqHealthChecks(
        this IServiceCollection services,
        Action<CroniqHealthCheckOptions>? configure = null)
    {
        ArgumentNullException.ThrowIfNull(services);

        services.AddOptions<CroniqHealthCheckOptions>();
        if (configure is not null)
        {
            services.Configure(configure);
        }

        services.AddHealthChecks()
            .AddCheck<CroniqPersistenceHealthCheck>(
                "croniq.persistence",
                failureStatus: HealthStatus.Unhealthy,
                tags: new[] { "croniq", "ready" })
            .AddCheck<CroniqTriggerHealthCheck>(
                "croniq.triggers",
                failureStatus: HealthStatus.Degraded,
                tags: new[] { "croniq", "ready" });

        return services;
    }

    public static IServiceCollection AddCroniqJob<TJob>(this IServiceCollection services)
        where TJob : class, IJob
    {
        return AddCroniqJob(services, typeof(TJob));
    }

    public static IServiceCollection AddCroniqJob(this IServiceCollection services, Type jobType)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(jobType);

        if (!typeof(IJob).IsAssignableFrom(jobType))
        {
            throw new InvalidOperationException($"Type {jobType.FullName ?? jobType.Name} must implement IJob.");
        }

        var attribute = jobType.GetCustomAttribute<CroniqJobAttribute>();
        if (attribute is null)
        {
            throw new InvalidOperationException(
                $"Type {jobType.FullName ?? jobType.Name} is missing [CroniqJob]. " +
                "Add [CroniqJob(\"namespace\", \"name\")] or register via AddCroniqJob(namespace, name, handler).");
        }

        services.AddTransient(jobType);
        services.AddSingleton<JobRegistration>(new JobRegistration(jobType));
        return services;
    }

    public static IServiceCollection AddCroniqJobsFromAssembly(this IServiceCollection services, Assembly assembly)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(assembly);

        var errors = new List<string>();
        var jobs = CollectCroniqJobTypes(assembly, errors);
        var duplicates = jobs
            .GroupBy(job => BuildAttributeKey(job.Attribute), StringComparer.OrdinalIgnoreCase)
            .Where(group => group.Count() > 1)
            .ToArray();

        if (duplicates.Length > 0)
        {
            foreach (var duplicate in duplicates)
            {
                var types = string.Join(", ", duplicate.Select(entry => entry.JobType.FullName ?? entry.JobType.Name));
                errors.Add($"Job '{duplicate.Key}' is declared by multiple types: {types}. Use unique [CroniqJob] values.");
            }
        }

        if (errors.Count > 0)
        {
            throw new InvalidOperationException("Croniq job scan failed:\n" + string.Join("\n", errors));
        }

        foreach (var job in jobs)
        {
            services.AddCroniqJob(job.JobType);
        }

        return services;
    }

    public static IServiceCollection AddCroniqJobsFromEntryAssembly(this IServiceCollection services)
    {
        ArgumentNullException.ThrowIfNull(services);

        var assembly = Assembly.GetEntryAssembly();
        if (assembly is null)
        {
            throw new InvalidOperationException("Entry assembly could not be resolved. Use AddCroniqJobsFromAssembly(...) instead.");
        }

        return services.AddCroniqJobsFromAssembly(assembly);
    }

    private static IReadOnlyList<ScannedJob> CollectCroniqJobTypes(Assembly assembly, List<string> errors)
    {
        var jobs = new List<ScannedJob>();
        Type[] types;

        try
        {
            types = assembly.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            types = ex.Types.Where(type => type is not null).Select(type => type!).ToArray();
            if (ex.LoaderExceptions is { Length: > 0 })
            {
                foreach (var loaderException in ex.LoaderExceptions)
                {
                    if (loaderException is not null)
                    {
                        errors.Add(loaderException.Message);
                    }
                }
            }
        }

        foreach (var type in types)
        {
            var attribute = type.GetCustomAttribute<CroniqJobAttribute>();
            if (attribute is null)
            {
                continue;
            }

            if (!type.IsClass || type.IsAbstract)
            {
                errors.Add($"Type {type.FullName ?? type.Name} is marked with [CroniqJob] but is not a concrete class.");
                continue;
            }

            if (type.ContainsGenericParameters)
            {
                errors.Add($"Type {type.FullName ?? type.Name} is marked with [CroniqJob] but is an open generic type.");
                continue;
            }

            if (!typeof(IJob).IsAssignableFrom(type))
            {
                errors.Add($"Type {type.FullName ?? type.Name} is marked with [CroniqJob] but does not implement IJob.");
                continue;
            }

            jobs.Add(new ScannedJob(type, attribute));
        }

        return jobs;
    }

    private static string BuildAttributeKey(CroniqJobAttribute attribute)
    {
        if (string.IsNullOrWhiteSpace(attribute.Variant))
        {
            return $"{attribute.NamespaceSegment}:{attribute.JobName}";
        }

        return $"{attribute.NamespaceSegment}:{attribute.JobName}:{attribute.Variant}";
    }

    private static bool ValidateCroniqOptions(CroniqOptions options)
    {
        if (options is null)
        {
            return false;
        }

        if (string.IsNullOrWhiteSpace(options.EnvironmentTag) || string.IsNullOrWhiteSpace(options.InstanceId))
        {
            return false;
        }

        return !string.IsNullOrWhiteSpace(options.TenantId);
    }

    private static bool ValidateWorkerHostOptions(WorkerHostOptions options)
    {
        return options.BatchSize > 0
            && options.IdleDelay >= TimeSpan.Zero
            && options.BusyDelay >= TimeSpan.Zero
            && options.ErrorDelay >= TimeSpan.Zero
            && options.LeaseRenewalLeadTime >= TimeSpan.Zero;
    }

    private static bool ValidateSeedingOptions(CroniqSeedingOptions options)
    {
        return string.IsNullOrWhiteSpace(options.Mode)
            || Enum.TryParse<CroniqSeedingMode>(options.Mode, ignoreCase: true, out _);
    }

    private static bool ValidateJobRegistrySyncOptions(CroniqJobRegistrySyncOptions options)
    {
        return string.IsNullOrWhiteSpace(options.Mode)
            || Enum.TryParse<CroniqSeedingMode>(options.Mode, ignoreCase: true, out _);
    }

    private static bool ValidateStartupOptions(CroniqStartupOptions options)
    {
        return CroniqStartupModeParser.TryParse(options.Mode, out _);
    }

    private sealed record ScannedJob(Type JobType, CroniqJobAttribute Attribute);
}
