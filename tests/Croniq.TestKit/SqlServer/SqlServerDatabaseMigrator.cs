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
    }

    public static async Task ResetDatabaseAsync(string connectionString, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));

        await ApplyMigrationsAsync(connectionString, cancellationToken).ConfigureAwait(false);

        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
        DELETE FROM [croniq].[WebhookSecretHistory];
        DELETE FROM [croniq].[WebhookEndpointEvents];
        DELETE FROM [croniq].[WebhookDeadLetters];
        DELETE FROM [croniq].[WebhookEndpoints];
        DELETE FROM [croniq].[DeadLetters];
        DELETE FROM [croniq].[Triggers];
        DELETE FROM [croniq].[Jobs];
        DELETE FROM [croniq].[ApiKeys];
        DELETE FROM [croniq].[ApiClients];
        """;

        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
    }

    private static ServiceProvider BuildProvider(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(builder =>
        {
            builder.AddSimpleConsole(options => options.SingleLine = true);
            builder.SetMinimumLevel(LogLevel.Warning);
        });
        services.AddCroniqSqlServerDbContext(options =>
        {
            options.ConnectionString = connectionString;
            options.MigrationsAssembly ??= typeof(SqlServerDbContext).Assembly.GetName().Name;
        });

        return services.BuildServiceProvider();
    }
}
