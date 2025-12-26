using System;
using System.Linq;
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
            throw new InvalidOperationException($"No EF Core migrations were discovered for '{migrationsAssembly.Assembly.GetName().Name}'.");
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
        IF OBJECT_ID(N'[croniq].[WebhookEndpoints]', 'U') IS NOT NULL DELETE FROM [croniq].[WebhookEndpoints];
        IF OBJECT_ID(N'[croniq].[DeadLetters]', 'U') IS NOT NULL DELETE FROM [croniq].[DeadLetters];
        IF OBJECT_ID(N'[croniq].[Triggers]', 'U') IS NOT NULL DELETE FROM [croniq].[Triggers];
        IF OBJECT_ID(N'[croniq].[Jobs]', 'U') IS NOT NULL DELETE FROM [croniq].[Jobs];
        IF OBJECT_ID(N'[croniq].[Runners]', 'U') IS NOT NULL DELETE FROM [croniq].[Runners];
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

        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        IF NOT EXISTS (SELECT 1 FROM [croniq].[Tenants] WHERE [TenantId] = @tenantId)
        BEGIN
            INSERT INTO [croniq].[Tenants] ([TenantId], [Name], [IsActive])
            VALUES (@tenantId, @name, 1);
        END
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        command.Parameters.AddWithValue("@tenantId", resolvedTenantId);
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
            options.UseSqlServer(builder.ConnectionString, sql =>
            {
                sql.MigrationsAssembly(typeof(SqlServerDbContext).Assembly.GetName().Name);
            });
        });

        return services.BuildServiceProvider();
    }
}
