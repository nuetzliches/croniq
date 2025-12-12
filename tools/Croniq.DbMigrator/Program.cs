using Croniq.Data.SqlServer;
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using System.Linq;

var connectionString = Environment.GetEnvironmentVariable("CRONIQ_SQL_CONNECTION");
if (string.IsNullOrWhiteSpace(connectionString))
{
    Console.Error.WriteLine("CRONIQ_SQL_CONNECTION environment variable is required.");
    return 1;
}

using var cancellation = new CancellationTokenSource(TimeSpan.FromMinutes(10));
var token = cancellation.Token;

var services = new ServiceCollection();
services.AddLogging(builder => builder.AddSimpleConsole());
services.AddCroniqSqlServerDbContext(options =>
{
    options.ConnectionString = connectionString;
    options.EnableDetailedErrors = true;
    options.EnableSensitiveDataLogging = true;
});

await using var provider = services.BuildServiceProvider();

try
{
    await ApplyMigrationsAsync(provider, token).ConfigureAwait(false);
    Console.WriteLine("Croniq SQL Server migrations applied successfully.");
    return 0;
}
catch (Exception ex)
{
    Console.Error.WriteLine($"Failed to apply migrations: {ex}");
    return 1;
}

static async Task ApplyMigrationsAsync(IServiceProvider provider, CancellationToken token)
{
    var loggerFactory = provider.GetRequiredService<ILoggerFactory>();
    var logger = loggerFactory.CreateLogger("Croniq.DbMigrator");
    const int maxAttempts = 5;
    var delay = TimeSpan.FromSeconds(5);
    var attempt = 0;

    while (true)
    {
        attempt++;
        using var scope = provider.CreateScope();
        var context = scope.ServiceProvider.GetRequiredService<SqlServerDbContext>();

        try
        {
            var pendingMigrations = await context.Database.GetPendingMigrationsAsync(token).ConfigureAwait(false);
            if (!pendingMigrations.Any())
            {
                logger.LogInformation("Croniq SQL Server schema is already up to date.");
                return;
            }

            await context.Database.MigrateAsync(token).ConfigureAwait(false);
            return;
        }
        catch (SqlException ex) when (IsDatabaseProvisioningError(ex) && attempt < maxAttempts)
        {
            logger.LogWarning(ex, "Croniq SQL Server database not ready (attempt {Attempt}/{MaxAttempts}). Retrying in {DelaySeconds}s...", attempt, maxAttempts, delay.TotalSeconds);
            await Task.Delay(delay, token).ConfigureAwait(false);
            delay = TimeSpan.FromSeconds(Math.Min(delay.TotalSeconds * 2, 30));
        }
        catch (SqlException ex) when (IsDatabaseProvisioningError(ex))
        {
            logger.LogError(ex, "Croniq SQL Server database unavailable after {Attempts} attempts.", attempt);
            throw;
        }
    }
}

static bool IsDatabaseProvisioningError(SqlException exception)
{
    foreach (SqlError error in exception.Errors)
    {
        if (error.Number is 4060 or 1801 or 18456)
        {
            return true;
        }
    }

    return false;
}
