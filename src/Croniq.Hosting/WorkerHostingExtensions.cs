using System;
using Croniq.Core;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.Data.SqlServer;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.SqlServer;
using Croniq.Providers.Default;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Hosting;

public static class WorkerHostingExtensions
{
    public static IServiceCollection AddCroniqWorkerServices(this IServiceCollection services, IConfiguration configuration)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configuration);

        services.Configure<CroniqOptions>(configuration.GetSection("Croniq:Core"));
        services.Configure<WorkerHostOptions>(configuration.GetSection("Croniq:WorkerHost"));
        services.Configure<InMemoryJobStoreOptions>(configuration.GetSection("Croniq:JobStore:InMemory"));
        services.Configure<MisfirePolicyOptions>(configuration.GetSection("Croniq:Policies:Misfire"));
        services.Configure<ExecutionPolicyOptions>(configuration.GetSection("Croniq:Policies:Execution"));
        services.Configure<PolicyOverrideOptions>(configuration.GetSection("Croniq:Policies:Overrides"));
        services.Configure<CroniqPersistenceOptions>(configuration.GetSection("Croniq:Persistence"));
        services.Configure<SqlServerOptions>(configuration.GetSection("Croniq:SqlServer"));

        services.AddCroniqCore();
        services.AddCroniqDefaultProviders();
        services.AddCroniqInMemoryJobStore();
        services.AddCroniqWorkerHost();

        var persistenceOpts = configuration.GetSection("Croniq:Persistence").Get<CroniqPersistenceOptions>() ?? new CroniqPersistenceOptions();
        var sharedSqlServer = configuration.GetSection("Croniq:SqlServer").Get<SqlServerOptions>() ?? new SqlServerOptions();

        if (string.Equals(persistenceOpts.Mode, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            var conn = ResolveConnectionString(
                persistenceOpts.SqlServer.ConnectionString,
                sharedSqlServer.ConnectionString,
                configuration);

            if (string.IsNullOrWhiteSpace(conn))
            {
                throw new InvalidOperationException("Croniq:Persistence:SqlServer:ConnectionString or Croniq:SqlServer:ConnectionString is required when Persistence.Mode = SqlServer.");
            }

            services.AddCroniqSqlServerPersistence(sqlOptions =>
            {
                sqlOptions.ConnectionString = conn;
                sqlOptions.MigrationsAssembly = persistenceOpts.SqlServer.MigrationsAssembly ?? sharedSqlServer.MigrationsAssembly;
                sqlOptions.EnableDetailedErrors = persistenceOpts.SqlServer.EnableDetailedErrors ?? sharedSqlServer.EnableDetailedErrors;
                sqlOptions.EnableSensitiveDataLogging = persistenceOpts.SqlServer.EnableSensitiveDataLogging ?? sharedSqlServer.EnableSensitiveDataLogging;
            }, persistenceOptions =>
            {
                if (persistenceOpts.SqlServer.LeaseDurationSeconds.HasValue)
                {
                    persistenceOptions.LeaseDurationSeconds = persistenceOpts.SqlServer.LeaseDurationSeconds.Value;
                }

                if (persistenceOpts.SqlServer.DeadLetterRetentionDays.HasValue)
                {
                    persistenceOptions.DeadLetterRetentionDays = persistenceOpts.SqlServer.DeadLetterRetentionDays.Value;
                }

                if (persistenceOpts.SqlServer.DeadLetterReasonMaxLength.HasValue)
                {
                    persistenceOptions.DeadLetterReasonMaxLength = persistenceOpts.SqlServer.DeadLetterReasonMaxLength.Value;
                }
            });
        }

        return services;
    }

    private static string? ResolveConnectionString(string? domainSpecific, string? shared, IConfiguration configuration)
    {
        return domainSpecific
            ?? shared
            ?? configuration.GetConnectionString("CroniqSqlServer")
            ?? configuration.GetConnectionString("Croniq")
            ?? configuration.GetConnectionString("DefaultConnection");
    }
}
