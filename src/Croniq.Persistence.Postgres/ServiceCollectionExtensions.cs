using System.IO;
using Croniq.Data.Postgres;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Persistence.Postgres;

/// <summary>
/// DI helpers for wiring EF Core backed persistence.
/// </summary>
public static class ServiceCollectionExtensions
{
    private const string DataProtectionSectionName = "Croniq:Security:DataProtection";
    private const string DefaultDataProtectionAppName = "Croniq";

    public static IServiceCollection AddCroniqPostgresPersistence(
        this IServiceCollection services,
        IConfiguration configuration,
        string sectionName = "Croniq:Postgres")
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configuration is null) throw new ArgumentNullException(nameof(configuration));

        var section = configuration.GetSection(sectionName);
        services.Configure<PostgresOptions>(section);
        services.Configure<PostgresPersistenceOptions>(section);

        return AddCroniqPostgresPersistenceInternal(
            services,
            options => section.Bind(options),
            configurePersistence: null,
            configuration);
    }

    public static IServiceCollection AddCroniqPostgresPersistence(
        this IServiceCollection services,
        Action<PostgresOptions> configurePostgres,
        Action<PostgresPersistenceOptions>? configurePersistence = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configurePostgres is null) throw new ArgumentNullException(nameof(configurePostgres));

        return AddCroniqPostgresPersistenceInternal(services, configurePostgres, configurePersistence, configuration: null);
    }

    private static IServiceCollection AddCroniqPostgresPersistenceInternal(
        IServiceCollection services,
        Action<PostgresOptions> configurePostgres,
        Action<PostgresPersistenceOptions>? configurePersistence,
        IConfiguration? configuration)
    {
        ConfigureDataProtection(services, configuration);
        services.AddCroniqPostgresDbContext(configurePostgres);
        services.AddOptions<PostgresPersistenceOptions>();
        services.AddOptions<WorkerStoreOptions>();
        services.AddOptions<RunnerStoreOptions>();
        if (configurePersistence is not null)
        {
            services.Configure(configurePersistence);
        }

        services.AddSingleton<IJobPersistenceProvider, PostgresJobPersistenceProvider>();
        services.AddSingleton<IJobStore>(sp => (IJobStore)sp.GetRequiredService<IJobPersistenceProvider>());
        services.AddSingleton<ICalendarStore>(sp => (ICalendarStore)sp.GetRequiredService<IJobPersistenceProvider>());
        services.AddSingleton<IJobDeadLetterStore, PostgresJobDeadLetterStore>();
        services.AddSingleton<IWebhookPersistenceProvider, PostgresWebhookPersistenceProvider>();
        services.AddSingleton<IWebhookDeadLetterStore, PostgresWebhookDeadLetterStore>();
        services.AddSingleton<PostgresWebhookActivityStore>();
        services.AddSingleton<IWebhookActivityStore>(sp => sp.GetRequiredService<PostgresWebhookActivityStore>());
        services.AddSingleton<IWebhookActivityRecorder>(sp => sp.GetRequiredService<PostgresWebhookActivityStore>());
        services.AddSingleton<IWebhookIngressEventStore, PostgresWebhookIngressEventStore>();
        services.AddSingleton<IWebhookEndpointChangefeed, PostgresWebhookEndpointChangefeed>();
        services.AddSingleton<IWorkerStore, PostgresWorkerStore>();
        services.AddSingleton<IRunnerStore, PostgresRunnerStore>();
        services.AddSingleton<IWorkItemStore, PostgresWorkItemStore>();

        return services;
    }

    private static void ConfigureDataProtection(IServiceCollection services, IConfiguration? configuration)
    {
        var builder = services.AddDataProtection();
        var section = configuration?.GetSection(DataProtectionSectionName);
        var keyRingPath = section?.GetValue<string>("KeyRingPath");
        var applicationName = section?.GetValue<string>("ApplicationName");

        if (!string.IsNullOrWhiteSpace(keyRingPath))
        {
            builder.PersistKeysToFileSystem(new DirectoryInfo(keyRingPath));
        }

        var resolvedName = string.IsNullOrWhiteSpace(applicationName)
            ? DefaultDataProtectionAppName
            : applicationName;

        builder.SetApplicationName(resolvedName);
    }
}
