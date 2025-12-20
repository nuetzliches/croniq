using System;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Auth.SqlServer;
using Croniq.Core;
using Croniq.Options;
using Croniq.Core.Policies;
using Croniq.Data.SqlServer;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Providers.Default;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq.Hosting;

public static class PlatformHostingExtensions
{
    public static IServiceCollection AddCroniqPlatformServices(this IServiceCollection services, IConfiguration configuration)
    {
        services.Configure<CroniqOptions>(configuration.GetSection("Croniq:Core"));
        services.Configure<CroniqStartupOptions>(configuration.GetSection("Croniq:Startup"));
        services.Configure<CroniqAuthOptions>(configuration.GetSection("Croniq:Auth"));
        services.Configure<CroniqOidcOptions>(configuration.GetSection("Croniq:Auth:Oidc"));
        services.Configure<CroniqTokenOptions>(configuration.GetSection("Croniq:Auth:Tokens"));
        services.Configure<PasswordAuthOptions>(configuration.GetSection("Croniq:Auth:Password"));
        services.Configure<CroniqPersistenceOptions>(configuration.GetSection("Croniq:Persistence"));
        services.Configure<SqlServerOptions>(configuration.GetSection("Croniq:SqlServer"));
        services.Configure<MisfirePolicyOptions>(configuration.GetSection("Croniq:Policies:Misfire"));
        services.Configure<ExecutionPolicyOptions>(configuration.GetSection("Croniq:Policies:Execution"));
        services.Configure<PolicyOverrideOptions>(configuration.GetSection("Croniq:Policies:Overrides"));

        services.AddCroniqCore();
        services.AddCroniqDefaultProviders();

        var authOpts = configuration.GetSection("Croniq:Auth").Get<CroniqAuthOptions>() ?? new CroniqAuthOptions();
        var persistenceOpts = configuration.GetSection("Croniq:Persistence").Get<CroniqPersistenceOptions>() ?? new CroniqPersistenceOptions();
        var sharedSqlServer = configuration.GetSection("Croniq:SqlServer").Get<SqlServerOptions>() ?? new SqlServerOptions();

        services.AddCroniqInMemoryJobStore();

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

        if (string.Equals(authOpts.Mode, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            var conn = ResolveConnectionString(
                authOpts.SqlServer.ConnectionString,
                sharedSqlServer.ConnectionString,
                configuration);

            if (string.IsNullOrWhiteSpace(conn))
            {
                throw new InvalidOperationException("Croniq:Auth:SqlServer:ConnectionString or Croniq:SqlServer:ConnectionString is required when Auth.Mode = SqlServer.");
            }

            services.AddCroniqAuthSqlServer(sqlOptions =>
            {
                sqlOptions.ConnectionString = conn;
                sqlOptions.MigrationsAssembly = authOpts.SqlServer.MigrationsAssembly ?? sharedSqlServer.MigrationsAssembly;
                sqlOptions.EnableDetailedErrors = authOpts.SqlServer.EnableDetailedErrors ?? sharedSqlServer.EnableDetailedErrors;
                sqlOptions.EnableSensitiveDataLogging = authOpts.SqlServer.EnableSensitiveDataLogging ?? sharedSqlServer.EnableSensitiveDataLogging;
            });
        }
        else
        {
            var apiKey = authOpts.InMemory.ApiKey;
            if (string.IsNullOrWhiteSpace(apiKey))
            {
                throw new InvalidOperationException("Croniq:Auth:InMemory:ApiKey must be set when Auth.Mode = InMemory.");
            }

            services.AddCroniqAuthCore(options =>
            {
                options.ApiKeys.Add(new ApiKeySeed(
                    KeyId: "default",
                    Secret: apiKey,
                    TenantId: authOpts.InMemory.TenantId,
                    EnvironmentTag: authOpts.InMemory.EnvironmentTag,
                    Scopes: new[]
                    {
                        CroniqScopes.SchedulesWrite,
                        CroniqScopes.SchedulesDeadLetter,
                        CroniqScopes.JobsRead,
                        CroniqScopes.JobsTrigger,
                        CroniqScopes.WebhooksRead,
                        CroniqScopes.WebhooksWrite,
                        CroniqScopes.WebhooksRotate,
                        CroniqScopes.WebhooksDeadLetter,
                        CroniqScopes.ApiKeysManage,
                        CroniqScopes.TenantsAdmin
                    },
                    ClientId: "default"));

                options.Tenants.Add(new TenantSeed(
                    TenantId: authOpts.InMemory.TenantId,
                    Reference: authOpts.InMemory.TenantId,
                    Name: $"{authOpts.InMemory.TenantId} (in-memory)",
                    IsActive: true,
                    CreatedAtUtc: DateTimeOffset.UtcNow));
            });
        }

        services.TryAddScoped<ICallerContextAccessor, CallerContextAccessor>();
        services.TryAddScoped<ICallerContextFactory, CallerContextFactory>();

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
