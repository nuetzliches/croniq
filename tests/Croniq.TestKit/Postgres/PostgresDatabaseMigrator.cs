using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.Postgres;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Npgsql;

namespace Croniq.TestKit.Postgres;

/// <summary>
/// Applies EF Core migrations and resets Croniq Postgres state for contract tests.
/// </summary>
public static class PostgresDatabaseMigrator
{
    public static async Task ApplyMigrationsAsync(string connectionString, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));

        await using var provider = BuildProvider(connectionString);
        await using var scope = provider.CreateAsyncScope();
        var services = scope.ServiceProvider;
        var context = services.GetRequiredService<PostgresDbContext>();
        var logger = services
            .GetRequiredService<ILoggerFactory>()
            .CreateLogger("Croniq.TestKit.Postgres.PostgresDatabaseMigrator");
        var migrationsAssembly = context.GetService<IMigrationsAssembly>();
        if (migrationsAssembly.Migrations.Count == 0)
        {
            LogMigrationDiagnostics(logger, context, migrationsAssembly, DescribeConnection(connectionString));
            logger.LogWarning(
                "No EF Core migrations were discovered for '{AssemblyName}'. Falling back to EnsureCreated for tests.",
                migrationsAssembly.Assembly.GetName().Name);
            await context.Database.EnsureCreatedAsync(cancellationToken).ConfigureAwait(false);
            return;
        }
        if (logger.IsEnabled(LogLevel.Debug))
        {
            logger.LogDebug("Migrator loaded migrations: {MigrationNames}", string.Join(", ", migrationsAssembly.Migrations.Keys));
        }
        var allMigrations = context.Database.GetMigrations();
        var appliedMigrations = context.Database.GetAppliedMigrations();
        var pendingMigrations = context.Database.GetPendingMigrations();
        if (logger.IsEnabled(LogLevel.Debug))
        {
            logger.LogDebug(
                "Discovered {AllCount} migrations. Applied: {AppliedCount}. Pending: {PendingCount}. First pending: {FirstPending}",
                allMigrations.Count(),
                appliedMigrations.Count(),
                pendingMigrations.Count(),
                pendingMigrations.FirstOrDefault() ?? "<none>");
        }
        await context.Database.MigrateAsync(cancellationToken).ConfigureAwait(false);
    }

    public static async Task ResetDatabaseAsync(string connectionString, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));

        await ApplyMigrationsAsync(connectionString, cancellationToken).ConfigureAwait(false);

        await using var connection = new NpgsqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        DELETE FROM "auth"."RefreshTokens";
        DELETE FROM "auth"."Users";
        DELETE FROM "croniq"."WebhookSecretHistory";
        DELETE FROM "croniq"."WebhookEndpointEvents";
        DELETE FROM "croniq"."WebhookDeadLetters";
        DELETE FROM "croniq"."WebhookIngressEvents";
        DELETE FROM "croniq"."WebhookEndpointIpRules";
        DELETE FROM "croniq"."WebhookEndpoints";
        DELETE FROM "croniq"."WorkClaims";
        DELETE FROM "croniq"."WorkItems";
        DELETE FROM "croniq"."DeadLetters";
        DELETE FROM "croniq"."Triggers";
        DELETE FROM "croniq"."Jobs";
        DELETE FROM "croniq"."RunnerCapabilities";
        DELETE FROM "croniq"."Runners";
        DELETE FROM "croniq"."WorkerInstances";
        DELETE FROM "croniq"."ApiKeys";
        DELETE FROM "croniq"."ApiClients";
        DELETE FROM "croniq"."Calendars";
        DELETE FROM "croniq"."Tenants";
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    public static async Task EnsureTenantExistsAsync(
        string connectionString,
        string tenantId,
        string? name = null,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required.", nameof(tenantId));

        await ApplyMigrationsAsync(connectionString, cancellationToken).ConfigureAwait(false);

        var resolvedTenantId = tenantId.Trim();
        var resolvedName = string.IsNullOrWhiteSpace(name) ? resolvedTenantId : name.Trim();
        var resolvedReference = resolvedTenantId;

        await using var connection = new NpgsqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        INSERT INTO "croniq"."Tenants" ("TenantId", "Reference", "Name", "IsActive")
        VALUES (@tenantId, @reference, @name, TRUE)
        ON CONFLICT ("TenantId") DO NOTHING;
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        command.Parameters.AddWithValue("tenantId", resolvedTenantId);
        command.Parameters.AddWithValue("reference", resolvedReference);
        command.Parameters.AddWithValue("name", resolvedName);
        await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    private static ServiceProvider BuildProvider(string connectionString)
    {
        var builder = new NpgsqlConnectionStringBuilder(connectionString)
        {
            ApplicationName = "Croniq.TestKit.PostgresMigrator"
        };

        var services = new ServiceCollection();
        services.AddLogging(builder =>
        {
            builder.AddSimpleConsole(options => options.SingleLine = true);
            builder.SetMinimumLevel(LogLevel.Warning);
        });
        services.AddDbContext<PostgresDbContext>(options =>
        {
            options.UseNpgsql(builder.ConnectionString);
        });

        return services.BuildServiceProvider();
    }

    private static void LogMigrationDiagnostics(
        ILogger logger,
        DbContext context,
        IMigrationsAssembly migrationsAssembly,
        string connectionSummary)
    {
        var contextAssembly = context.GetType().Assembly;
        var migrationsAsm = migrationsAssembly.Assembly;
        var contextLocation = SafeAssemblyLocation(contextAssembly);
        var migrationsLocation = SafeAssemblyLocation(migrationsAsm);

        logger.LogWarning(
            "Migration diagnostics: ContextAssembly={ContextAssembly} Location={ContextLocation} LoadContext={ContextLoadContext}",
            contextAssembly.FullName,
            contextLocation,
            DescribeLoadContext(contextAssembly));
        logger.LogWarning(
            "Migration diagnostics: MigrationsAssembly={MigrationsAssembly} Location={MigrationsLocation} LoadContext={MigrationsLoadContext} LocationExists={LocationExists}",
            migrationsAsm.FullName,
            migrationsLocation,
            DescribeLoadContext(migrationsAsm),
            File.Exists(migrationsLocation));
        logger.LogWarning(
            "Migration diagnostics: ProviderName={ProviderName} BaseDirectory={BaseDirectory} CurrentDirectory={CurrentDirectory}",
            context.Database.ProviderName ?? "<unknown>",
            AppContext.BaseDirectory,
            Environment.CurrentDirectory);
        logger.LogWarning(
            "Migration diagnostics: OptionsMigrationsAssembly={OptionsMigrationsAssembly}",
            ResolveMigrationsAssemblyOption(context) ?? "<not-configured>");
        logger.LogWarning(
            "Migration diagnostics: Connection={ConnectionSummary}",
            connectionSummary);

        var migrationTypes = GetMigrationTypeNames(migrationsAsm, context.GetType(), logger);
        logger.LogWarning(
            "Migration diagnostics: MigrationTypeCount={MigrationTypeCount} Types={MigrationTypes}",
            migrationTypes.Count,
            migrationTypes.Count == 0 ? "<none>" : string.Join(", ", migrationTypes));

        var loadedCroniqAssemblies = AppDomain.CurrentDomain.GetAssemblies()
            .Where(assembly => assembly.GetName().Name?.StartsWith("Croniq", StringComparison.OrdinalIgnoreCase) == true)
            .Select(assembly => $"{assembly.GetName().Name} ({SafeAssemblyLocation(assembly)})")
            .OrderBy(name => name, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        logger.LogWarning(
            "Migration diagnostics: LoadedCroniqAssemblies={Assemblies}",
            loadedCroniqAssemblies.Length == 0 ? "<none>" : string.Join(", ", loadedCroniqAssemblies));
    }

    private static string SafeAssemblyLocation(Assembly assembly)
    {
        try
        {
            return string.IsNullOrWhiteSpace(assembly.Location) ? "<none>" : assembly.Location;
        }
        catch (NotSupportedException)
        {
            return "<dynamic>";
        }
    }

    private static string DescribeLoadContext(Assembly assembly)
    {
        return AssemblyLoadContext.GetLoadContext(assembly)?.Name ?? "<default>";
    }

    private static string? ResolveMigrationsAssemblyOption(DbContext context)
    {
        var options = context.GetService<IDbContextOptions>();
        var extension = options.Extensions.FirstOrDefault(ext =>
            ext.GetType().Name.Contains("RelationalOptionsExtension", StringComparison.OrdinalIgnoreCase));
        if (extension is null)
        {
            return null;
        }

        var property = extension.GetType().GetProperty("MigrationsAssembly", BindingFlags.Instance | BindingFlags.Public);
        return property?.GetValue(extension) as string;
    }

    private static IReadOnlyList<string> GetMigrationTypeNames(Assembly assembly, Type contextType, ILogger logger)
    {
        try
        {
            return assembly
                .GetTypes()
                .Where(type => typeof(Migration).IsAssignableFrom(type) && !type.IsAbstract)
                .Select(type =>
                {
                    var hasContext = type
                        .GetCustomAttributes<DbContextAttribute>()
                        .Any(attribute => attribute.ContextType == contextType);
                    return $"{type.FullName}{(hasContext ? " [context]" : " [no-context]")}";
                })
                .OrderBy(name => name, StringComparer.OrdinalIgnoreCase)
                .ToArray();
        }
        catch (ReflectionTypeLoadException ex)
        {
            logger.LogWarning(ex, "Migration diagnostics: Failed to load migration types from {AssemblyName}.", assembly.FullName);
            foreach (var loaderException in ex.LoaderExceptions ?? Array.Empty<Exception>())
            {
                logger.LogWarning(loaderException, "Migration diagnostics: Loader exception.");
            }

            return Array.Empty<string>();
        }
    }

    private static string DescribeConnection(string connectionString)
    {
        try
        {
            var builder = new NpgsqlConnectionStringBuilder(connectionString);
            return $"Host={builder.Host};Database={builder.Database}";
        }
        catch (ArgumentException)
        {
            return "<unavailable>";
        }
    }
}
