using System;
using System.Reflection;
using Croniq.Auth.SqlServer;
using Croniq.Core;
using Croniq.Core.Hosting;
using Croniq.Options;
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
        services.Configure<CroniqStartupOptions>(configuration.GetSection("Croniq:Startup"));
        services.Configure<WorkerHostOptions>(configuration.GetSection("Croniq:WorkerHost"));
        services.Configure<InMemoryJobStoreOptions>(configuration.GetSection("Croniq:JobStore:InMemory"));
        services.Configure<CroniqSeedingOptions>(configuration.GetSection("Croniq:Seeding"));
        services.Configure<CroniqJobRegistrySyncOptions>(configuration.GetSection("Croniq:JobRegistrySync"));
        services.Configure<MisfirePolicyOptions>(configuration.GetSection("Croniq:Policies:Misfire"));
        services.Configure<ExecutionPolicyOptions>(configuration.GetSection("Croniq:Policies:Execution"));
        services.Configure<PolicyOverrideOptions>(configuration.GetSection("Croniq:Policies:Overrides"));
        services.Configure<CroniqPersistenceOptions>(configuration.GetSection("Croniq:Persistence"));
        services.Configure<SqlServerOptions>(configuration.GetSection("Croniq:SqlServer"));
        services.Configure<CroniqRetentionOptions>(configuration.GetSection("Croniq:Retention"));

        services.AddCroniqCore();
        services.AddCroniqDefaultProviders();
        services.AddCroniqInMemoryJobStore();
        services.AddHostedService<CroniqJobRegistrySyncHostedService>();
        services.AddHostedService<CroniqTriggerSeedingHostedService>();
        services.AddHostedService<CroniqTriggerSummaryHostedService>();
        services.AddCroniqWorkerHost();

        var persistenceOpts = configuration.GetSection("Croniq:Persistence").Get<CroniqPersistenceOptions>() ?? new CroniqPersistenceOptions();
        var sharedSqlServer = configuration.GetSection("Croniq:SqlServer").Get<SqlServerOptions>() ?? new SqlServerOptions();

        if (string.Equals(persistenceOpts.Mode, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            var conn = ResolveConnectionString(
                persistenceOpts.SqlServer.ConnectionString,
                sharedSqlServer.ConnectionString,
                configuration);
            var commandTimeoutSeconds = persistenceOpts.SqlServer.CommandTimeoutSeconds
                ?? sharedSqlServer.CommandTimeoutSeconds;

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
                sqlOptions.CommandTimeoutSeconds = commandTimeoutSeconds;
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

            var retention = configuration.GetSection("Croniq:Retention").Get<CroniqRetentionOptions>() ?? new CroniqRetentionOptions();
            if (retention.Enabled)
            {
                services.AddCroniqJob<RetentionCleanupJob>();

                var attribute = typeof(RetentionCleanupJob).GetCustomAttribute<Croniq.Sdk.CroniqJobAttribute>();
                if (attribute is null)
                {
                    throw new InvalidOperationException("RetentionCleanupJob is missing [CroniqJob] attribute.");
                }

                var triggerId = string.IsNullOrWhiteSpace(retention.TriggerId)
                    ? "croniq.retention.cleanup"
                    : retention.TriggerId.Trim();

                services.AddSingleton(new CroniqTriggerSeedRegistration(attribute, retention.ScheduleCron)
                {
                    TriggerId = triggerId,
                    ManagedBy = "croniq-retention",
                    TimeZoneId = retention.TimeZoneId,
                    Enabled = true
                });
            }
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
