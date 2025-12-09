using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
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
        var context = scope.ServiceProvider.GetRequiredService<SqlServerDbContext>();
        await context.Database.MigrateAsync(cancellationToken).ConfigureAwait(false);
    }

    public static async Task ResetDatabaseAsync(string connectionString, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(connectionString)) throw new ArgumentException("Connection string is required.", nameof(connectionString));

        await using var connection = new SqlConnection(connectionString);
        await connection.OpenAsync(cancellationToken).ConfigureAwait(false);

        const string sql = """
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
        services.AddLogging(builder => builder.AddSimpleConsole());
        services.AddCroniqSqlServerDbContext(options =>
        {
            options.ConnectionString = connectionString;
            options.EnableDetailedErrors = true;
            options.EnableSensitiveDataLogging = true;
        });

        return services.BuildServiceProvider();
    }
}
