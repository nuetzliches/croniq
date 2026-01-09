using Croniq.Data.SqlServer;
using Croniq.Auth.Abstractions;
using Croniq.Auth.SqlServer;
using Croniq.Options;
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;
using Microsoft.AspNetCore.Identity;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using System.Globalization;
using System.Linq;
using System.Reflection;

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
    options.EnableSensitiveDataLogging = false;
    options.EnableDetailedErrors = true;
    options.MigrationsAssembly = typeof(SqlServerDbContext).Assembly.GetName().Name;
});

services.AddSingleton<ITenantStore, SqlServerTenantStore>();
services.AddSingleton<IPasswordUserStore, SqlServerPasswordUserStore>();

await using var provider = services.BuildServiceProvider();

try
{
    await ApplyMigrationsAsync(provider, connectionString, token).ConfigureAwait(false);
    Console.WriteLine("Croniq SQL Server migrations applied successfully.");

    try
    {
        await SeedAdminAsync(provider, token).ConfigureAwait(false);
    }
    catch (Exception ex)
    {
        // Seeding is a dev convenience. It should not block containers/CI by default.
        // If you want seeding failures to fail the migrator, set CRONIQ_SEED_ADMIN_REQUIRED=true.
        var required = Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_REQUIRED");
        var isRequired = string.Equals(required, "true", StringComparison.OrdinalIgnoreCase)
            || string.Equals(required, "1", StringComparison.OrdinalIgnoreCase);

        Console.Error.WriteLine($"Admin seeding failed: {ex}");
        if (isRequired)
        {
            throw;
        }
    }
    return 0;
}
catch (Exception ex)
{
    Console.Error.WriteLine($"Failed to apply migrations: {ex}");
    return 1;
}

static async Task ApplyMigrationsAsync(IServiceProvider provider, string connectionString, CancellationToken token)
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
            await EnsureDatabaseExistsAsync(connectionString, logger, token).ConfigureAwait(false);
            await EnsureSchemasAsync(connectionString, token).ConfigureAwait(false);
            var migrationsAssembly = context.GetService<IMigrationsAssembly>();
            if (migrationsAssembly.Migrations.Count == 0)
            {
                throw new InvalidOperationException(
                    $"No EF Core migrations were discovered for '{migrationsAssembly.Assembly.GetName().Name}'.");
            }
            var pendingMigrations = await context.Database.GetPendingMigrationsAsync(token).ConfigureAwait(false);
            if (!pendingMigrations.Any())
            {
                logger.LogInformation("Croniq SQL Server schema is already up to date.");
                return;
            }

            try
            {
                await context.Database.MigrateAsync(token).ConfigureAwait(false);
            }
            catch (SqlException ex) when (IsObjectAlreadyExistsError(ex))
            {
                // Migration squashing support:
                // If the database already contains Croniq tables from older migrations (kept in the volume),
                // applying the new squashed baseline migration would fail with "object already exists".
                // In that case we "baseline" by inserting the new migration id into __EFMigrationsHistory.
                if (!await TryBaselineSquashedInitialCreateAsync(connectionString, pendingMigrations, logger, token).ConfigureAwait(false))
                {
                    throw;
                }
            }
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

static async Task EnsureDatabaseExistsAsync(string connectionString, ILogger logger, CancellationToken token)
{
    var builder = new SqlConnectionStringBuilder(connectionString);
    var databaseName = builder.InitialCatalog;
    if (string.IsNullOrWhiteSpace(databaseName))
    {
        logger.LogWarning("Croniq SQL connection string has no database name; skipping database creation.");
        return;
    }

    if (string.Equals(databaseName, "master", StringComparison.OrdinalIgnoreCase))
    {
        return;
    }

    var masterBuilder = new SqlConnectionStringBuilder(builder.ConnectionString)
    {
        InitialCatalog = "master"
    };

    await using var connection = new SqlConnection(masterBuilder.ConnectionString);
    await connection.OpenAsync(token).ConfigureAwait(false);

    var exists = await ExecuteScalarAsync<int?>(
        connection,
        "SELECT DB_ID(@name);",
        token,
        new SqlParameter("@name", databaseName)).ConfigureAwait(false);
    if (exists is not null)
    {
        return;
    }

    logger.LogInformation("Croniq SQL Server database '{Database}' not found. Creating...", databaseName);
    try
    {
        await ExecuteNonQueryAsync(
            connection,
            "DECLARE @sql nvarchar(max) = N'CREATE DATABASE ' + QUOTENAME(@name); EXEC(@sql);",
            token,
            new SqlParameter("@name", databaseName)).ConfigureAwait(false);
        logger.LogInformation("Croniq SQL Server database '{Database}' created.", databaseName);
    }
    catch (SqlException ex) when (IsDatabaseAlreadyExistsError(ex))
    {
        logger.LogInformation("Croniq SQL Server database '{Database}' already exists.", databaseName);
    }
}

static async Task EnsureSchemasAsync(string connectionString, CancellationToken token)
{
    var builder = new SqlConnectionStringBuilder(connectionString);
    if (string.IsNullOrWhiteSpace(builder.InitialCatalog)
        || string.Equals(builder.InitialCatalog, "master", StringComparison.OrdinalIgnoreCase))
    {
        return;
    }

    await using var connection = new SqlConnection(connectionString);
    await connection.OpenAsync(token).ConfigureAwait(false);

    await ExecuteNonQueryAsync(
        connection,
        "IF SCHEMA_ID(N'croniq') IS NULL EXEC(N'CREATE SCHEMA [croniq]');"
        + "IF SCHEMA_ID(N'auth') IS NULL EXEC(N'CREATE SCHEMA [auth]');",
        token).ConfigureAwait(false);
}

static bool IsObjectAlreadyExistsError(SqlException exception)
{
    foreach (SqlError error in exception.Errors)
    {
        if (error.Number is 2714)
        {
            return true;
        }
    }

    return false;
}

static bool IsDatabaseAlreadyExistsError(SqlException exception)
{
    foreach (SqlError error in exception.Errors)
    {
        if (error.Number is 1801)
        {
            return true;
        }
    }

    return false;
}

static async Task<bool> TryBaselineSquashedInitialCreateAsync(
    string connectionString,
    IEnumerable<string> pendingMigrations,
    ILogger logger,
    CancellationToken token)
{
    var pending = pendingMigrations as string[] ?? pendingMigrations.ToArray();
    if (pending.Length != 1)
    {
        return false;
    }

    var migrationId = pending[0];
    if (!migrationId.EndsWith("_InitialCreate", StringComparison.OrdinalIgnoreCase))
    {
        return false;
    }

    await using var connection = new SqlConnection(connectionString);
    await connection.OpenAsync(token).ConfigureAwait(false);

    // Only baseline if this looks like an existing Croniq database.
    var tenantsTableId = await ExecuteScalarAsync<int?>(
        connection,
        "SELECT OBJECT_ID(N'[croniq].[Tenants]');",
        token).ConfigureAwait(false);
    if (tenantsTableId is null)
    {
        return false;
    }

    var historyTableId = await ExecuteScalarAsync<int?>(
        connection,
        "SELECT OBJECT_ID(N'[__EFMigrationsHistory]');",
        token).ConfigureAwait(false);
    if (historyTableId is null)
    {
        return false;
    }

    var historyCount = await ExecuteScalarAsync<int>(
        connection,
        "SELECT COUNT(*) FROM [__EFMigrationsHistory];",
        token).ConfigureAwait(false);
    if (historyCount == 0)
    {
        return false;
    }

    var exists = await ExecuteScalarAsync<int>(
        connection,
        "SELECT CASE WHEN EXISTS (SELECT 1 FROM [__EFMigrationsHistory] WHERE [MigrationId] = @id) THEN 1 ELSE 0 END;",
        token,
        new SqlParameter("@id", migrationId)).ConfigureAwait(false);
    if (exists == 1)
    {
        logger.LogInformation("Baseline migration '{MigrationId}' is already recorded in __EFMigrationsHistory.", migrationId);
        return true;
    }

    var productVersion = typeof(DbContext).Assembly.GetName().Version?.ToString(3) ?? "unknown";
    logger.LogWarning(
        "Detected existing Croniq schema with legacy migration history. Recording squashed baseline migration '{MigrationId}' (ProductVersion={ProductVersion}) without applying DDL.",
        migrationId,
        productVersion);

    await ExecuteNonQueryAsync(
        connection,
        "INSERT INTO [__EFMigrationsHistory] ([MigrationId], [ProductVersion]) VALUES (@id, @pv);",
        token,
        new SqlParameter("@id", migrationId),
        new SqlParameter("@pv", productVersion)).ConfigureAwait(false);

    return true;
}

static async Task<T> ExecuteScalarAsync<T>(SqlConnection connection, string sql, CancellationToken token, params SqlParameter[] parameters)
{
    await using var command = connection.CreateCommand();
    command.CommandText = sql;
    foreach (var parameter in parameters)
    {
        command.Parameters.Add(parameter);
    }

    var result = await command.ExecuteScalarAsync(token).ConfigureAwait(false);
    if (result is null || result is DBNull)
    {
        return default!;
    }

    if (result is T typed)
    {
        return typed;
    }

    var targetType = Nullable.GetUnderlyingType(typeof(T)) ?? typeof(T);
    if (targetType.IsEnum)
    {
        return (T)Enum.ToObject(targetType, result);
    }

    return (T)Convert.ChangeType(result, targetType, CultureInfo.InvariantCulture);
}

static async Task ExecuteNonQueryAsync(SqlConnection connection, string sql, CancellationToken token, params SqlParameter[] parameters)
{
    await using var command = connection.CreateCommand();
    command.CommandText = sql;
    foreach (var parameter in parameters)
    {
        command.Parameters.Add(parameter);
    }

    await command.ExecuteNonQueryAsync(token).ConfigureAwait(false);
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

    var tenantId = ResolveEnv("CRONIQ_SEED_TENANT_ID")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_ID")
        ?? new CroniqOptions().TenantId.Trim();
    var tenantName = ResolveEnv("CRONIQ_SEED_TENANT_NAME")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_NAME")
        ?? tenantId;
    var tenantReference = ResolveEnv("CRONIQ_SEED_TENANT_REFERENCE")
        ?? tenantId;
    var username = (Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_USERNAME") ?? "admin").Trim();
    var password = (Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_PASSWORD") ?? "admin").Trim();
    var passwordChangeRequired = Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_PASSWORD_CHANGE_REQUIRED");
    var isPasswordChangeRequired = string.IsNullOrWhiteSpace(passwordChangeRequired)
        || string.Equals(passwordChangeRequired, "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(passwordChangeRequired, "1", StringComparison.OrdinalIgnoreCase);
    var overwrite = string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_OVERWRITE"), "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_OVERWRITE"), "1", StringComparison.OrdinalIgnoreCase);

    var seedScopesRaw = (Environment.GetEnvironmentVariable("CRONIQ_SEED_ADMIN_SCOPES") ?? string.Empty).Trim();

    if (string.IsNullOrWhiteSpace(tenantId))
    {
        logger.LogWarning("Admin seeding enabled but tenant id could not be resolved; set CRONIQ_SEED_TENANT_ID or CRONIQ_CORE_TENANT_ID. Skipping.");
        return;
    }

    if (string.IsNullOrWhiteSpace(username) || string.IsNullOrWhiteSpace(password))
    {
        logger.LogWarning("Admin seeding enabled but username/password missing; skipping.");
        return;
    }

    var tenant = await tenants.CreateAsync(new TenantCreateRequest(tenantName, tenantId, tenantReference), token).ConfigureAwait(false);

    var existing = await users.FindByUsernameAsync(tenant.TenantId, username, token).ConfigureAwait(false);
    if (existing is not null && !overwrite)
    {
        logger.LogInformation(
            "Admin user already exists for tenant '{TenantId}'; skipping (set CRONIQ_SEED_ADMIN_OVERWRITE=true to reset).",
            tenant.TenantId);
        return;
    }

    var scopes = ResolveSeedAdminScopes(seedScopesRaw, logger);
    logger.LogInformation("Admin seeding scopes: {Scopes}", string.Join(", ", scopes));

    // PasswordHasher does not incorporate the user object by default.
    var hasher = new PasswordHasher<object>();
    var hash = hasher.HashPassword(user: new object(), password);

    await users.UpsertAsync(new PasswordUserUpsertRequest(
        tenant.TenantId,
        username,
        hash,
        scopes,
        IsActive: true,
        PasswordChangeRequired: isPasswordChangeRequired), token).ConfigureAwait(false);

    logger.LogInformation(
        "Seeded admin user '{Username}' for tenant '{TenantId}' (PasswordChangeRequired={PasswordChangeRequired}).",
        username,
        tenant.TenantId,
        isPasswordChangeRequired);
}

static IReadOnlyCollection<string> ResolveSeedAdminScopes(string seedScopesRaw, ILogger logger)
{
    var fallback = new[]
    {
        CroniqScopes.SchedulesWrite,
        CroniqScopes.SchedulesDeadLetter,
        CroniqScopes.JobsRead,
        CroniqScopes.JobsWrite,
        CroniqScopes.JobsTrigger,
        CroniqScopes.WorkPoll,
        CroniqScopes.WorkRenew,
        CroniqScopes.WorkAck,
        CroniqScopes.WorkEvents,
        CroniqScopes.WorkersHeartbeat,
        CroniqScopes.WorkersRead,
        CroniqScopes.RunnersHeartbeat,
        CroniqScopes.RunnersRead,
        CroniqScopes.ExecutionsRead,
        CroniqScopes.WebhooksRead,
        CroniqScopes.WebhooksWrite,
        CroniqScopes.WebhooksRotate,
        CroniqScopes.WebhooksDeadLetter,
        CroniqScopes.ApiKeysManage,
        CroniqScopes.TenantsAdmin
    };

    var allKnown = GetAllKnownScopes();

    if (string.IsNullOrWhiteSpace(seedScopesRaw))
    {
        return fallback;
    }

    if (string.Equals(seedScopesRaw, "all", StringComparison.OrdinalIgnoreCase))
    {
        logger.LogInformation("Admin seeding: CRONIQ_SEED_ADMIN_SCOPES=all -> granting all scopes ({ScopeCount}).", allKnown.Count);
        return allKnown;
    }

    var parsed = seedScopesRaw
        .Split(new[] { ' ', '\t', '\r', '\n', ',' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .OrderBy(scope => scope, StringComparer.OrdinalIgnoreCase)
        .ToArray();

    if (parsed.Length == 0)
    {
        logger.LogWarning("Admin seeding: CRONIQ_SEED_ADMIN_SCOPES was set but empty after parsing; falling back to default scope set.");
        return fallback;
    }

    logger.LogInformation("Admin seeding: using custom scope set ({ScopeCount}).", parsed.Length);
    return parsed;
}

static IReadOnlyCollection<string> GetAllKnownScopes()
{
    // Keep 'all' in sync with CroniqScopes without duplicating the list in multiple places.
    return typeof(CroniqScopes)
        .GetFields(BindingFlags.Public | BindingFlags.Static)
        .Where(field => field is { IsLiteral: true, IsInitOnly: false } && field.FieldType == typeof(string))
        .Select(field => (string)field.GetRawConstantValue()!)
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .OrderBy(scope => scope, StringComparer.OrdinalIgnoreCase)
        .ToArray();
}

static string? ResolveEnv(string name)
{
    var raw = Environment.GetEnvironmentVariable(name);
    if (string.IsNullOrWhiteSpace(raw))
    {
        return null;
    }

    return raw.Trim();
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
