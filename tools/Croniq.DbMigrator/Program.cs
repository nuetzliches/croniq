using Croniq.Data.SqlServer;
using Croniq.Auth.Abstractions;
using Croniq.Auth.SqlServer;
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
using Microsoft.AspNetCore.Identity;
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

services.AddSingleton<ITenantStore, SqlServerTenantStore>();
services.AddSingleton<IPasswordUserStore, SqlServerPasswordUserStore>();

await using var provider = services.BuildServiceProvider();

try
{
    await ApplyMigrationsAsync(provider, token).ConfigureAwait(false);
    Console.WriteLine("Croniq SQL Server migrations applied successfully.");

    await SeedAdminAsync(provider, token).ConfigureAwait(false);
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

static async Task SeedAdminAsync(IServiceProvider provider, CancellationToken token)
{
    var seedEnabled = Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN");
    if (!string.Equals(seedEnabled, "true", StringComparison.OrdinalIgnoreCase)
        && !string.Equals(seedEnabled, "1", StringComparison.OrdinalIgnoreCase))
    {
        return;
    }

    using var scope = provider.CreateScope();
    var loggerFactory = scope.ServiceProvider.GetRequiredService<ILoggerFactory>();
    var logger = loggerFactory.CreateLogger("Croniq.DbMigrator.Seed");

    var tenants = scope.ServiceProvider.GetRequiredService<ITenantStore>();
    var users = scope.ServiceProvider.GetRequiredService<IPasswordUserStore>();

    var tenantReference = (Environment.GetEnvironmentVariable("CRONIQ_SEED_TENANT_REFERENCE") ?? "default").Trim();
    var tenantName = (Environment.GetEnvironmentVariable("CRONIQ_SEED_TENANT_NAME") ?? "Default").Trim();
    var username = (Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_USERNAME") ?? "admin").Trim();
    var password = (Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_PASSWORD") ?? "admin").Trim();
    var overwrite = string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_OVERWRITE"), "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_OVERWRITE"), "1", StringComparison.OrdinalIgnoreCase);

    if (string.IsNullOrWhiteSpace(tenantReference))
    {
        logger.LogWarning("Admin seeding enabled but CRONIQ_SEED_TENANT_REFERENCE is empty; skipping.");
        return;
    }

    if (string.IsNullOrWhiteSpace(username) || string.IsNullOrWhiteSpace(password))
    {
        logger.LogWarning("Admin seeding enabled but username/password missing; skipping.");
        return;
    }

    var tenant = await tenants.CreateAsync(tenantReference, tenantName, token).ConfigureAwait(false);

    var existing = await users.FindByUsernameAsync(tenant.TenantId, username, token).ConfigureAwait(false);
    if (existing is not null && !overwrite)
    {
        logger.LogInformation("Admin user already exists for tenant '{TenantReference}'; skipping (set CRONIQ_SEED_ADMIN_OVERWRITE=true to reset).", tenant.Reference);
        return;
    }

    var scopes = new[]
    {
        CroniqScopes.SchedulesWrite,
        CroniqScopes.JobsRead,
        CroniqScopes.JobsWrite,
        CroniqScopes.JobsTrigger,
        CroniqScopes.ExecutionsRead,
        CroniqScopes.WebhooksRead,
        CroniqScopes.WebhooksWrite,
        CroniqScopes.WebhooksRotate,
        CroniqScopes.WebhooksDeadLetter,
        CroniqScopes.ApiKeysManage,
        CroniqScopes.TenantsAdmin
    };

    // PasswordHasher does not incorporate the user object by default.
    var hasher = new PasswordHasher<object>();
    var hash = hasher.HashPassword(user: new object(), password);

    await users.UpsertAsync(new PasswordUserUpsertRequest(
        tenant.TenantId,
        username,
        hash,
        scopes,
        IsActive: true,
        PasswordChangeRequired: true), token).ConfigureAwait(false);

    logger.LogInformation("Seeded admin user '{Username}' for tenant '{TenantReference}' (PasswordChangeRequired=true).", username, tenant.Reference);
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
