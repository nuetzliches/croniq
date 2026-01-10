using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.TestKit.SqlServer;

/// <summary>
/// Applies EF Core migrations and resets Croniq SQL Server state for contract tests.
/// </summary>
public static class SqlServerDatabaseMigrator
{
    public static async Task ApplyMigrationsAsync(string connectionString, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));

        await using var provider = BuildProvider(connectionString);
        await using var scope = provider.CreateAsyncScope();
        var services = scope.ServiceProvider;
        var context = services.GetRequiredService<SqlServerDbContext>();
        var logger = services
            .GetRequiredService<ILoggerFactory>()
            .CreateLogger("Croniq.TestKit.SqlServer.SqlServerDatabaseMigrator");
        var migrationsAssembly = context.GetService<IMigrationsAssembly>();
        if (migrationsAssembly.Migrations.Count == 0)
        {
            LogMigrationDiagnostics(logger, context, migrationsAssembly, connectionString);
            logger.LogWarning(
                "No EF Core migrations were discovered for '{AssemblyName}'. Falling back to EnsureCreated for tests.",
                migrationsAssembly.Assembly.GetName().Name);
            await context.Database.EnsureCreatedAsync(cancellationToken).ConfigureAwait(false);
            await EnsurePasswordChangeRequiredColumnAsync(connectionString, cancellationToken).ConfigureAwait(false);
            await EnsureWebhookIngressEventsTableAsync(connectionString, cancellationToken).ConfigureAwait(false);
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

        // Safety net: ensure critical auth columns exist even if a DB was created from
        // an older snapshot or migrations history got out of sync in CI.
        await EnsurePasswordChangeRequiredColumnAsync(connectionString, cancellationToken).ConfigureAwait(false);
        await EnsureWebhookIngressEventsTableAsync(connectionString, cancellationToken).ConfigureAwait(false);
    }

    private static async Task EnsurePasswordChangeRequiredColumnAsync(string connectionString, CancellationToken cancellationToken)
    {
        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        IF OBJECT_ID(N'[auth].[Users]', 'U') IS NOT NULL
           AND COL_LENGTH('auth.Users', 'PasswordChangeRequired') IS NULL
        BEGIN
            ALTER TABLE [auth].[Users]
            ADD [PasswordChangeRequired] bit NOT NULL CONSTRAINT [DF_auth_Users_PasswordChangeRequired] DEFAULT (0);
        END
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    private static async Task EnsureWebhookIngressEventsTableAsync(string connectionString, CancellationToken cancellationToken)
    {
        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        IF OBJECT_ID(N'[croniq].[WebhookIngressEvents]', 'U') IS NULL
        BEGIN
            IF SCHEMA_ID(N'croniq') IS NULL EXEC('CREATE SCHEMA [croniq]');

            CREATE TABLE [croniq].[WebhookIngressEvents] (
                [Id] bigint IDENTITY(1,1) NOT NULL,
                [EventId] nvarchar(64) NOT NULL,
                [HookKey] nvarchar(128) NOT NULL,
                [JobKey] nvarchar(256) NOT NULL,
                [TenantId] nvarchar(64) NOT NULL,
                [EnvironmentTag] nvarchar(64) NOT NULL,
                [Payload] nvarchar(max) NOT NULL,
                [HeadersJson] nvarchar(max) NULL,
                [MetadataJson] nvarchar(max) NULL,
                [ReceivedAtUtc] datetime2 NOT NULL,
                [Status] nvarchar(32) NOT NULL,
                [LeaseId] nvarchar(64) NULL,
                [LeaseExpiresAtUtc] datetime2 NULL,
                [AttemptCount] int NOT NULL,
                [LastError] nvarchar(1024) NULL,
                [CreatedAtUtc] datetime2 NOT NULL CONSTRAINT [DF_WebhookIngressEvents_CreatedAtUtc] DEFAULT (sysutcdatetime()),
                [UpdatedAtUtc] datetime2 NOT NULL CONSTRAINT [DF_WebhookIngressEvents_UpdatedAtUtc] DEFAULT (sysutcdatetime()),
                CONSTRAINT [PK_WebhookIngressEvents] PRIMARY KEY ([Id]),
                CONSTRAINT [FK_WebhookIngressEvents_Tenants_TenantId] FOREIGN KEY ([TenantId]) REFERENCES [croniq].[Tenants] ([TenantId]) ON DELETE NO ACTION
            );

            CREATE UNIQUE INDEX [IX_WebhookIngressEvents_EventId] ON [croniq].[WebhookIngressEvents] ([EventId]);
            CREATE INDEX [IX_WebhookIngressEvents_TenantId_EnvironmentTag_ReceivedAtUtc]
                ON [croniq].[WebhookIngressEvents] ([TenantId], [EnvironmentTag], [ReceivedAtUtc]);
            CREATE INDEX [IX_WebhookIngressEvents_TenantId_EnvironmentTag_Status_LeaseExpiresAtUtc]
                ON [croniq].[WebhookIngressEvents] ([TenantId], [EnvironmentTag], [Status], [LeaseExpiresAtUtc]);
        END
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    public static async Task ResetDatabaseAsync(string connectionString, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));

        await ApplyMigrationsAsync(connectionString, cancellationToken).ConfigureAwait(false);

        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        IF OBJECT_ID(N'[auth].[RefreshTokens]', 'U') IS NOT NULL DELETE FROM [auth].[RefreshTokens];
        IF OBJECT_ID(N'[auth].[Users]', 'U') IS NOT NULL DELETE FROM [auth].[Users];
        IF OBJECT_ID(N'[croniq].[WebhookSecretHistory]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookSecretHistory];
        IF OBJECT_ID(N'[croniq].[WebhookEndpointEvents]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookEndpointEvents];
        IF OBJECT_ID(N'[croniq].[WebhookDeadLetters]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookDeadLetters];
        IF OBJECT_ID(N'[croniq].[WebhookIngressEvents]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookIngressEvents];
        IF OBJECT_ID(N'[croniq].[WebhookEndpointIpRules]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookEndpointIpRules];
        IF OBJECT_ID(N'[croniq].[WebhookEndpoints]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookEndpoints];
        IF OBJECT_ID(N'[croniq].[WorkClaims]', 'U') IS NOT NULL DELETE FROM [croniq].[WorkClaims];
        IF OBJECT_ID(N'[croniq].[WorkItems]', 'U') IS NOT NULL DELETE FROM [croniq].[WorkItems];
        IF OBJECT_ID(N'[croniq].[DeadLetters]', 'U') IS NOT NULL DELETE FROM [croniq].[DeadLetters];
        IF OBJECT_ID(N'[croniq].[Triggers]', 'U') IS NOT NULL DELETE FROM [croniq].[Triggers];
        IF OBJECT_ID(N'[croniq].[Jobs]', 'U') IS NOT NULL DELETE FROM [croniq].[Jobs];
        IF OBJECT_ID(N'[croniq].[RunnerCapabilities]', 'U') IS NOT NULL DELETE FROM [croniq].[RunnerCapabilities];
        IF OBJECT_ID(N'[croniq].[Runners]', 'U') IS NOT NULL DELETE FROM [croniq].[Runners];
        IF OBJECT_ID(N'[croniq].[WorkerInstances]', 'U') IS NOT NULL DELETE FROM [croniq].[WorkerInstances];
        IF OBJECT_ID(N'[croniq].[ApiKeys]', 'U') IS NOT NULL DELETE FROM [croniq].[ApiKeys];
        IF OBJECT_ID(N'[croniq].[ApiClients]', 'U') IS NOT NULL DELETE FROM [croniq].[ApiClients];
        IF OBJECT_ID(N'[croniq].[Tenants]', 'U') IS NOT NULL DELETE FROM [croniq].[Tenants];
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

        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        IF NOT EXISTS (SELECT 1 FROM [croniq].[Tenants] WHERE [TenantId] = @tenantId)
        BEGIN
            INSERT INTO [croniq].[Tenants] ([TenantId], [Reference], [Name], [IsActive])
            VALUES (@tenantId, @reference, @name, 1);
        END
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        command.Parameters.AddWithValue("@tenantId", resolvedTenantId);
        command.Parameters.AddWithValue("@reference", resolvedReference);
        command.Parameters.AddWithValue("@name", resolvedName);
        await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    private static ServiceProvider BuildProvider(string connectionString)
    {
        var builder = new SqlConnectionStringBuilder(connectionString)
        {
            TrustServerCertificate = true,
            Encrypt = false,
            MultipleActiveResultSets = false,
            ApplicationName = "Croniq.TestKit.SqlServerMigrator"
        };

        var services = new ServiceCollection();
        services.AddLogging(builder =>
        {
            builder.AddSimpleConsole(options => options.SingleLine = true);
            builder.SetMinimumLevel(LogLevel.Warning);
        });
        // Important: do NOT enable SqlServer retry-on-failure for migrations.
        // If a transient error occurs mid-migration, retries can replay DDL (CREATE TABLE ...)
        // and fail with "There is already an object named ...".
        services.AddDbContext<SqlServerDbContext>(options =>
        {
            // Let EF Core use the DbContext's assembly for migrations to avoid test ALC mismatches.
            options.UseSqlServer(builder.ConnectionString);
        });

        return services.BuildServiceProvider();
    }

    private static void LogMigrationDiagnostics(
        ILogger logger,
        SqlServerDbContext context,
        IMigrationsAssembly migrationsAssembly,
        string connectionString)
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
            DescribeConnection(connectionString));

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
            var builder = new SqlConnectionStringBuilder(connectionString);
            return $"DataSource={builder.DataSource};InitialCatalog={builder.InitialCatalog}";
        }
        catch (ArgumentException)
        {
            return "<unavailable>";
        }
    }
}
