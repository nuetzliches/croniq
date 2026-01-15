using System;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;
using Npgsql;
using Npgsql.EntityFrameworkCore.PostgreSQL;

namespace Croniq.Data.Postgres;

/// <summary>
/// DI helpers for wiring the shared Croniq Postgres DbContext.
/// </summary>
public static class DependencyInjectionExtensions
{
    public static IServiceCollection AddCroniqPostgresDbContext(
        this IServiceCollection services,
        Action<PostgresOptions> configure)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configure is null) throw new ArgumentNullException(nameof(configure));

        services.Configure(configure);

        void ConfigureContext(IServiceProvider sp, DbContextOptionsBuilder builder)
        {
            var options = sp.GetRequiredService<IOptions<PostgresOptions>>().Value
                ?? throw new InvalidOperationException("Postgres options were not configured.");
            if (string.IsNullOrWhiteSpace(options.ConnectionString))
            {
                throw new InvalidOperationException("Croniq Postgres connection string is required.");
            }

            var connectionString = options.ConnectionString;
            if (!string.IsNullOrWhiteSpace(options.SearchPath))
            {
                var connectionStringBuilder = new NpgsqlConnectionStringBuilder(connectionString)
                {
                    SearchPath = options.SearchPath
                };
                connectionString = connectionStringBuilder.ConnectionString;
            }

            builder.UseNpgsql(connectionString, sqlOptions =>
            {
                sqlOptions.EnableRetryOnFailure();
                if (!string.IsNullOrWhiteSpace(options.MigrationsAssembly))
                {
                    sqlOptions.MigrationsAssembly(options.MigrationsAssembly);
                }
                if (options.CommandTimeoutSeconds is > 0)
                {
                    sqlOptions.CommandTimeout(options.CommandTimeoutSeconds);
                }
            });

            builder.EnableDetailedErrors(options.EnableDetailedErrors);
            builder.EnableSensitiveDataLogging(options.EnableSensitiveDataLogging);
        }

        services.AddDbContext<PostgresDbContext>(
            ConfigureContext,
            contextLifetime: ServiceLifetime.Scoped,
            optionsLifetime: ServiceLifetime.Singleton);
        services.AddDbContextFactory<PostgresDbContext>(ConfigureContext, ServiceLifetime.Singleton);

        return services;
    }
}
