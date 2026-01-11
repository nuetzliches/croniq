using System;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Data.SqlServer;

/// <summary>
/// DI helpers for wiring the shared Croniq SQL Server DbContext.
/// </summary>
public static class DependencyInjectionExtensions
{
    public static IServiceCollection AddCroniqSqlServerDbContext(
        this IServiceCollection services,
        Action<SqlServerOptions> configure)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configure is null) throw new ArgumentNullException(nameof(configure));

        services.Configure(configure);

        void ConfigureContext(IServiceProvider sp, DbContextOptionsBuilder builder)
        {
            var options = sp.GetRequiredService<IOptions<SqlServerOptions>>().Value
                ?? throw new InvalidOperationException("SqlServer options were not configured.");
            if (string.IsNullOrWhiteSpace(options.ConnectionString))
            {
                throw new InvalidOperationException("Croniq SQL Server connection string is required.");
            }

            builder.UseSqlServer(options.ConnectionString, sqlOptions =>
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

            if (options.SuppressMarsSavepointWarning)
            {
                builder.ConfigureWarnings(warnings =>
                    // EF Core warning: "Savepoints are disabled because Multiple Active Result Sets (MARS) is enabled".
                    // We intentionally suppress it in tests to keep output clean.
                    // 30004 is the EventId emitted for this warning by EF Core.
                    warnings.Ignore(new EventId(30004, "SavepointsDisabledBecauseOfMARS")));
            }
        }

        services.AddDbContext<SqlServerDbContext>(
            ConfigureContext,
            contextLifetime: ServiceLifetime.Scoped,
            optionsLifetime: ServiceLifetime.Singleton);
        services.AddDbContextFactory<SqlServerDbContext>(ConfigureContext, ServiceLifetime.Singleton);

        return services;
    }
}
