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
    using var scope = provider.CreateScope();
    var context = scope.ServiceProvider.GetRequiredService<SqlServerDbContext>();
    await EnsureDatabaseCreatedAsync(context, token).ConfigureAwait(false);

    var pendingMigrations = await context.Database.GetPendingMigrationsAsync(token).ConfigureAwait(false);
    if (!pendingMigrations.Any())
    {
        Console.WriteLine("Croniq SQL Server schema is already up to date.");
        return;
    }

    await context.Database.MigrateAsync(token).ConfigureAwait(false);
}

static async Task EnsureDatabaseCreatedAsync(SqlServerDbContext context, CancellationToken token)
{
    try
    {
        var created = await context.Database.EnsureCreatedAsync(token).ConfigureAwait(false);
        if (created)
        {
            Console.WriteLine("Croniq SQL Server database created.");
        }
    }
    catch (SqlException ex) when (ex.Number == 1801)
    {
        Console.WriteLine("Croniq SQL Server database already exists. Skipping creation.");
    }
}
