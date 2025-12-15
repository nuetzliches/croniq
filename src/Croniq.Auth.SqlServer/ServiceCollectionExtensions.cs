using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Data.SqlServer;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Auth.SqlServer;

/// <summary>
/// DI helpers for SQL Server backed auth services.
/// </summary>
public static class ServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqAuthSqlServer(
        this IServiceCollection services,
        IConfiguration configuration,
        string sectionName = "Croniq:SqlServer")
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configuration is null) throw new ArgumentNullException(nameof(configuration));

        var section = configuration.GetSection(sectionName);
        services.Configure<SqlServerOptions>(section);
        return services.AddCroniqAuthSqlServer(options => section.Bind(options));
    }

    public static IServiceCollection AddCroniqAuthSqlServer(
        this IServiceCollection services,
        Action<SqlServerOptions> configureSql)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configureSql is null) throw new ArgumentNullException(nameof(configureSql));

        services.AddCroniqSqlServerDbContext(configureSql);
        services.AddSingleton<IApiKeyStore, SqlServerApiKeyStore>();
        services.AddOptions<CroniqTokenOptions>();
        services.AddSingleton<ICroniqTokenIssuer, CroniqTokenIssuer>();
        return services;
    }
}
