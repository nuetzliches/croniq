using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Data.Postgres;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Auth.Postgres;

/// <summary>
/// DI helpers for Postgres-backed auth services.
/// </summary>
public static class ServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqAuthPostgres(
        this IServiceCollection services,
        IConfiguration configuration,
        string sectionName = "Croniq:Postgres")
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configuration is null) throw new ArgumentNullException(nameof(configuration));

        var section = configuration.GetSection(sectionName);
        services.Configure<PostgresOptions>(section);
        return services.AddCroniqAuthPostgres(options => section.Bind(options));
    }

    public static IServiceCollection AddCroniqAuthPostgres(
        this IServiceCollection services,
        Action<PostgresOptions> configurePostgres)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configurePostgres is null) throw new ArgumentNullException(nameof(configurePostgres));

        services.AddCroniqPostgresDbContext(configurePostgres);
        services.AddSingleton<IApiKeyStore, PostgresApiKeyStore>();
        services.AddSingleton<ITenantStore, PostgresTenantStore>();

        services.AddOptions<PasswordAuthOptions>();
        services.AddSingleton<IPasswordUserStore, PostgresPasswordUserStore>();
        services.AddSingleton<IRefreshTokenStore, PostgresRefreshTokenStore>();
        services.AddSingleton<PasswordAuthService>();
        services.AddSingleton<IPasswordAuthService>(sp => sp.GetRequiredService<PasswordAuthService>());

        services.AddOptions<CroniqTokenOptions>();
        services.AddSingleton<ICroniqTokenIssuer, CroniqTokenIssuer>();
        return services;
    }
}
