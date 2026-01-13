using System.IO;
using Croniq.Data.SqlServer;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Persistence.SqlServer;

/// <summary>
/// DI helpers for wiring EF Core backed persistence.
/// </summary>
public static class ServiceCollectionExtensions
{
    private const string DataProtectionSectionName = "Croniq:Security:DataProtection";
    private const string DefaultDataProtectionAppName = "Croniq";

    public static IServiceCollection AddCroniqSqlServerPersistence(
        this IServiceCollection services,
        IConfiguration configuration,
        string sectionName = "Croniq:SqlServer")
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configuration is null) throw new ArgumentNullException(nameof(configuration));

        var section = configuration.GetSection(sectionName);
        services.Configure<SqlServerOptions>(section);
        services.Configure<SqlServerPersistenceOptions>(section);

        return AddCroniqSqlServerPersistenceInternal(
            services,
            options => section.Bind(options),
            configurePersistence: null,
            configuration);
    }

    public static IServiceCollection AddCroniqSqlServerPersistence(
        this IServiceCollection services,
        Action<SqlServerOptions> configureSql,
        Action<SqlServerPersistenceOptions>? configurePersistence = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configureSql is null) throw new ArgumentNullException(nameof(configureSql));

        return AddCroniqSqlServerPersistenceInternal(services, configureSql, configurePersistence, configuration: null);
    }

    private static IServiceCollection AddCroniqSqlServerPersistenceInternal(
        IServiceCollection services,
        Action<SqlServerOptions> configureSql,
        Action<SqlServerPersistenceOptions>? configurePersistence,
        IConfiguration? configuration)
    {
        ConfigureDataProtection(services, configuration);
        services.AddCroniqSqlServerDbContext(configureSql);
        services.AddOptions<SqlServerPersistenceOptions>();
        services.AddOptions<WorkerStoreOptions>();
        services.AddOptions<RunnerStoreOptions>();
        if (configurePersistence is not null)
        {
            services.Configure(configurePersistence);
        }

        services.AddSingleton<IJobPersistenceProvider, SqlServerJobPersistenceProvider>();
        services.AddSingleton<IJobStore>(sp => (IJobStore)sp.GetRequiredService<IJobPersistenceProvider>());
        services.AddSingleton<IJobDeadLetterStore, SqlServerJobDeadLetterStore>();
        services.AddSingleton<IWebhookPersistenceProvider, SqlServerWebhookPersistenceProvider>();
        services.AddSingleton<IWebhookDeadLetterStore, SqlServerWebhookDeadLetterStore>();
        services.AddSingleton<IWebhookIngressEventStore, SqlServerWebhookIngressEventStore>();
        services.AddSingleton<IWebhookEndpointChangefeed, SqlServerWebhookEndpointChangefeed>();
        services.AddSingleton<IWorkerStore, SqlServerWorkerStore>();
        services.AddSingleton<IRunnerStore, SqlServerRunnerStore>();
        services.AddSingleton<IWorkItemStore, SqlServerWorkItemStore>();

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

        services.PostConfigure<DataProtectionOptions>(options =>
        {
            if (string.IsNullOrWhiteSpace(options.ApplicationDiscriminator))
            {
                options.ApplicationDiscriminator = resolvedName;
            }
        });
    }
}
