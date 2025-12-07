using System;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
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
            });

            builder.EnableDetailedErrors(options.EnableDetailedErrors);
            builder.EnableSensitiveDataLogging(options.EnableSensitiveDataLogging);
        }

        services.AddDbContext<SqlServerDbContext>(ConfigureContext);
        services.AddDbContextFactory<SqlServerDbContext>(ConfigureContext);

        return services;
    }
}
