using Croniq.Data.SqlServer;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Persistence.SqlServer;

/// <summary>
/// DI helpers for wiring EF Core backed persistence.
/// </summary>
public static class ServiceCollectionExtensions
{
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

        return services.AddCroniqSqlServerPersistence(options => section.Bind(options));
    }

    public static IServiceCollection AddCroniqSqlServerPersistence(
        this IServiceCollection services,
        Action<SqlServerOptions> configureSql,
        Action<SqlServerPersistenceOptions>? configurePersistence = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configureSql is null) throw new ArgumentNullException(nameof(configureSql));

        services.AddCroniqSqlServerDbContext(configureSql);
        services.AddOptions<SqlServerPersistenceOptions>();
        if (configurePersistence is not null)
        {
            services.Configure(configurePersistence);
        }

        services.AddSingleton<IJobPersistenceProvider, SqlServerJobPersistenceProvider>();
        services.AddSingleton<IJobStore>(sp => (IJobStore)sp.GetRequiredService<IJobPersistenceProvider>());
        services.AddSingleton<IWebhookPersistenceProvider, SqlServerWebhookPersistenceProvider>();
        services.AddSingleton<IWebhookDeadLetterStore, SqlServerWebhookDeadLetterStore>();

        return services;
    }
}
