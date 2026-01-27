using Croniq.Core;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Postgres;
using Croniq.Auth.SqlServer;
using Croniq.Data.Postgres;
using Croniq.Data.SqlServer;
using Croniq.Options;
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;
using Microsoft.AspNetCore.Identity;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using System.Collections.Generic;
using System.ComponentModel;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Npgsql;
using PostgresApiClientEntity = Croniq.Data.Postgres.Entities.ApiClientEntity;
using PostgresApiKeyEntity = Croniq.Data.Postgres.Entities.ApiKeyEntity;
using SqlServerApiClientEntity = Croniq.Data.SqlServer.Entities.ApiClientEntity;
using SqlServerApiKeyEntity = Croniq.Data.SqlServer.Entities.ApiKeyEntity;

DatabaseProvider databaseProvider;
string connectionString;
try
{
    (databaseProvider, connectionString) = ResolveProvider();
}
catch (InvalidOperationException ex)
{
    Console.Error.WriteLine(ex.Message);
    return 1;
}

using var cancellation = new CancellationTokenSource(TimeSpan.FromMinutes(10));
var token = cancellation.Token;

var builder = Host.CreateApplicationBuilder();
builder.Configuration.AddEnvironmentVariables();

var configuration = builder.Configuration;
var services = builder.Services;
services.AddCroniqObservability(configuration, builder.Logging, "Croniq.DbMigrator", options =>
{
    if (string.IsNullOrWhiteSpace(configuration["Croniq:Observability:ConsoleLogFormat"]))
    {
        options.ConsoleLogFormat = "text";
    }

    if (string.IsNullOrWhiteSpace(configuration["Croniq:Observability:EnableTracing"]))
    {
        options.EnableTracing = false;
    }

    if (string.IsNullOrWhiteSpace(configuration["Croniq:Observability:EnableMetrics"]))
    {
        options.EnableMetrics = false;
    }
});
if (databaseProvider == DatabaseProvider.SqlServer)
{
    services.AddCroniqSqlServerDbContext(options =>
    {
        options.ConnectionString = connectionString;
        options.EnableSensitiveDataLogging = false;
        options.EnableDetailedErrors = true;
        options.MigrationsAssembly = typeof(SqlServerDbContext).Assembly.GetName().Name;
    });

    services.AddSingleton<ITenantStore, SqlServerTenantStore>();
    services.AddSingleton<IPasswordUserStore, SqlServerPasswordUserStore>();
}
else
{
    services.AddCroniqPostgresDbContext(options =>
    {
        options.ConnectionString = connectionString;
        options.EnableSensitiveDataLogging = false;
        options.EnableDetailedErrors = true;
        options.MigrationsAssembly = typeof(PostgresDbContext).Assembly.GetName().Name;
    });

    services.AddSingleton<ITenantStore, PostgresTenantStore>();
    services.AddSingleton<IPasswordUserStore, PostgresPasswordUserStore>();
}

await using var serviceProvider = services.BuildServiceProvider();

try
{
    if (databaseProvider == DatabaseProvider.SqlServer)
    {
        await ApplySqlServerMigrationsAsync(serviceProvider, connectionString, token).ConfigureAwait(false);
        Console.WriteLine("Croniq SQL Server migrations applied successfully.");
    }
    else
    {
        await ApplyPostgresMigrationsAsync(serviceProvider, connectionString, token).ConfigureAwait(false);
        Console.WriteLine("Croniq Postgres migrations applied successfully.");
    }

    try
    {
        await SeedTenantAsync(serviceProvider, token).ConfigureAwait(false);
    }
    catch (Exception ex)
    {
        // Seeding is a dev convenience. It should not block containers/CI by default.
        // If you want seeding failures to fail the migrator, set CRONIQ_SEED_TENANT_REQUIRED=true.
        var required = Environment.GetEnvironmentVariable("CRONIQ_SEED_TENANT_REQUIRED");
        var isRequired = string.Equals(required, "true", StringComparison.OrdinalIgnoreCase)
            || string.Equals(required, "1", StringComparison.OrdinalIgnoreCase);

        Console.Error.WriteLine($"Tenant seeding failed: {ex}");
        if (isRequired)
        {
            throw;
        }
    }

    try
    {
        await SeedAdminAsync(serviceProvider, token).ConfigureAwait(false);
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

    try
    {
        await SeedApiKeyAsync(serviceProvider, token).ConfigureAwait(false);
    }
    catch (Exception ex)
    {
        // Seeding is a dev convenience. It should not block containers/CI by default.
        // If you want seeding failures to fail the migrator, set CRONIQ_SEED_API_KEY_REQUIRED=true.
        var required = Environment.GetEnvironmentVariable("CRONIQ_SEED_API_KEY_REQUIRED");
        var isRequired = string.Equals(required, "true", StringComparison.OrdinalIgnoreCase)
            || string.Equals(required, "1", StringComparison.OrdinalIgnoreCase);

        Console.Error.WriteLine($"API key seeding failed: {ex}");
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

static async Task ApplySqlServerMigrationsAsync(IServiceProvider provider, string connectionString, CancellationToken token)
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
                LogMigrationDiagnostics(logger, context, migrationsAssembly, DescribeSqlConnection(connectionString));
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

static async Task ApplyPostgresMigrationsAsync(IServiceProvider provider, string connectionString, CancellationToken token)
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
        var context = scope.ServiceProvider.GetRequiredService<PostgresDbContext>();

        try
        {
            await EnsurePostgresDatabaseExistsAsync(connectionString, logger, token).ConfigureAwait(false);
            var migrationsAssembly = context.GetService<IMigrationsAssembly>();
            if (migrationsAssembly.Migrations.Count == 0)
            {
                LogMigrationDiagnostics(logger, context, migrationsAssembly, DescribePostgresConnection(connectionString));
                throw new InvalidOperationException(
                    $"No EF Core migrations were discovered for '{migrationsAssembly.Assembly.GetName().Name}'.");
            }
            var pendingMigrations = await context.Database.GetPendingMigrationsAsync(token).ConfigureAwait(false);
            if (!pendingMigrations.Any())
            {
                logger.LogInformation("Croniq Postgres schema is already up to date.");
                return;
            }

            await context.Database.MigrateAsync(token).ConfigureAwait(false);
            return;
        }
        catch (Exception ex) when (IsPostgresProvisioningError(ex) && attempt < maxAttempts)
        {
            logger.LogWarning(ex, "Croniq Postgres database not ready (attempt {Attempt}/{MaxAttempts}). Retrying in {DelaySeconds}s...", attempt, maxAttempts, delay.TotalSeconds);
            await Task.Delay(delay, token).ConfigureAwait(false);
            delay = TimeSpan.FromSeconds(Math.Min(delay.TotalSeconds * 2, 30));
        }
        catch (Exception ex) when (IsPostgresProvisioningError(ex))
        {
            logger.LogError(ex, "Croniq Postgres database unavailable after {Attempts} attempts.", attempt);
            throw;
        }
    }
}

static void LogMigrationDiagnostics(
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

static string SafeAssemblyLocation(Assembly assembly)
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

static string DescribeLoadContext(Assembly assembly)
{
    return AssemblyLoadContext.GetLoadContext(assembly)?.Name ?? "<default>";
}

static string? ResolveMigrationsAssemblyOption(DbContext context)
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

static IReadOnlyList<string> GetMigrationTypeNames(Assembly assembly, Type contextType, ILogger logger)
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

static string DescribeSqlConnection(string connectionString)
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

static string DescribePostgresConnection(string connectionString)
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

static async Task EnsurePostgresDatabaseExistsAsync(string connectionString, ILogger logger, CancellationToken token)
{
    var builder = new NpgsqlConnectionStringBuilder(connectionString);
    var databaseName = builder.Database;
    if (string.IsNullOrWhiteSpace(databaseName))
    {
        logger.LogWarning("Croniq Postgres connection string has no database name; skipping database creation.");
        return;
    }

    if (string.Equals(databaseName, "postgres", StringComparison.OrdinalIgnoreCase))
    {
        return;
    }

    var adminBuilder = new NpgsqlConnectionStringBuilder(builder.ConnectionString)
    {
        Database = "postgres"
    };

    await using var connection = new NpgsqlConnection(adminBuilder.ConnectionString);
    await connection.OpenAsync(token).ConfigureAwait(false);

    var exists = await ExecutePostgresScalarAsync<int?>(
        connection,
        "SELECT 1 FROM pg_database WHERE datname = @name;",
        token,
        new NpgsqlParameter("name", databaseName)).ConfigureAwait(false);
    if (exists is not null)
    {
        return;
    }

    logger.LogInformation("Croniq Postgres database '{Database}' not found. Creating...", databaseName);
    try
    {
        var safeName = databaseName.Replace("\"", "\"\"", StringComparison.Ordinal);
        await ExecutePostgresNonQueryAsync(
            connection,
            $"CREATE DATABASE \"{safeName}\";",
            token).ConfigureAwait(false);
        logger.LogInformation("Croniq Postgres database '{Database}' created.", databaseName);
    }
    catch (PostgresException ex) when (ex.SqlState == "42P04")
    {
        logger.LogInformation("Croniq Postgres database '{Database}' already exists.", databaseName);
    }
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

static async Task<T> ExecutePostgresScalarAsync<T>(NpgsqlConnection connection, string sql, CancellationToken token, params NpgsqlParameter[] parameters)
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

static async Task ExecutePostgresNonQueryAsync(NpgsqlConnection connection, string sql, CancellationToken token, params NpgsqlParameter[] parameters)
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

static async Task SeedTenantAsync(IServiceProvider provider, CancellationToken token)
{
    var seedEnabled = Environment.GetEnvironmentVariable("CRONIQ_SEED_TENANT");
    if (!string.Equals(seedEnabled, "true", StringComparison.OrdinalIgnoreCase)
        && !string.Equals(seedEnabled, "1", StringComparison.OrdinalIgnoreCase))
    {
        return;
    }

    using var scope = provider.CreateScope();
    var loggerFactory = scope.ServiceProvider.GetRequiredService<ILoggerFactory>();
    var logger = loggerFactory.CreateLogger("Croniq.DbMigrator.Seed");

    var tenants = scope.ServiceProvider.GetRequiredService<ITenantStore>();

    var tenantId = ResolveEnv("CRONIQ_SEED_TENANT_ID")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_ID")
        ?? new CroniqOptions().TenantId.Trim();
    var tenantName = ResolveEnv("CRONIQ_SEED_TENANT_NAME")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_NAME")
        ?? tenantId;
    var tenantReference = ResolveEnv("CRONIQ_SEED_TENANT_REFERENCE")
        ?? tenantId;

    if (string.IsNullOrWhiteSpace(tenantId))
    {
        logger.LogWarning("Tenant seeding enabled but tenant id could not be resolved; set CRONIQ_SEED_TENANT_ID or CRONIQ_CORE_TENANT_ID. Skipping.");
        return;
    }

    var tenant = await tenants.CreateAsync(new TenantCreateRequest(tenantName, tenantId, tenantReference), token).ConfigureAwait(false);
    logger.LogInformation("Seeded tenant '{TenantId}'.", tenant.TenantId);
}

static async Task SeedApiKeyAsync(IServiceProvider provider, CancellationToken token)
{
    using var scope = provider.CreateScope();
    var loggerFactory = scope.ServiceProvider.GetRequiredService<ILoggerFactory>();
    var logger = loggerFactory.CreateLogger("Croniq.DbMigrator.Seed");

    var list = ResolveEnv("CRONIQ_SEED_API_KEYS");
    if (!string.IsNullOrWhiteSpace(list))
    {
        var entries = ParseSeedApiKeyList(list, logger);
        foreach (var seedEntry in entries)
        {
            await SeedApiKeyEntryAsync(scope.ServiceProvider, seedEntry, logger, token).ConfigureAwait(false);
        }
        return;
    }

    var seedKey = ResolveEnv("CRONIQ_SEED_API_KEY");
    var seedKeyId = ResolveEnv("CRONIQ_SEED_API_KEY_ID");
    var seedSecret = ResolveEnv("CRONIQ_SEED_API_KEY_SECRET");

    var enabled = !string.IsNullOrWhiteSpace(seedKey)
        || (!string.IsNullOrWhiteSpace(seedKeyId) && !string.IsNullOrWhiteSpace(seedSecret));
    if (!enabled)
    {
        return;
    }

    if (string.IsNullOrWhiteSpace(seedKey) && !string.IsNullOrWhiteSpace(seedKeyId) && !string.IsNullOrWhiteSpace(seedSecret))
    {
        seedKey = string.Concat(seedKeyId, '.', seedSecret);
    }

    var (keyId, secret) = SplitSeedKey(seedKey ?? string.Empty);
    if (string.IsNullOrWhiteSpace(keyId) || string.IsNullOrWhiteSpace(secret))
    {
        logger.LogWarning("API key seeding enabled but CRONIQ_SEED_API_KEY is invalid; expected 'keyId.secret'. Skipping.");
        return;
    }

    var tenantId = ResolveEnv("CRONIQ_SEED_TENANT_ID")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_ID")
        ?? new CroniqOptions().TenantId.Trim();
    var tenantName = ResolveEnv("CRONIQ_SEED_TENANT_NAME")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_NAME")
        ?? tenantId;
    var tenantReference = ResolveEnv("CRONIQ_SEED_TENANT_REFERENCE")
        ?? tenantId;
    var environmentTag = ResolveEnv("CRONIQ_SEED_API_KEY_ENVIRONMENT")
        ?? ResolveEnv("CRONIQ_ENVIRONMENT")
        ?? new CroniqOptions().EnvironmentTag.Trim();
    var clientId = ResolveEnv("CRONIQ_SEED_API_KEY_CLIENT_ID")
        ?? ResolveEnv("CRONIQ_RUNNER_ID")
        ?? "default";
    var clientName = ResolveEnv("CRONIQ_SEED_API_KEY_NAME");
    var overwrite = string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_API_KEY_OVERWRITE"), "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_API_KEY_OVERWRITE"), "1", StringComparison.OrdinalIgnoreCase);

    if (string.IsNullOrWhiteSpace(tenantId) || string.IsNullOrWhiteSpace(clientId))
    {
        logger.LogWarning("API key seeding enabled but tenant/client id missing; set CRONIQ_SEED_TENANT_ID and CRONIQ_SEED_API_KEY_CLIENT_ID. Skipping.");
        return;
    }

    var singleEntry = (
        KeyId: keyId,
        Secret: secret,
        TenantId: tenantId,
        TenantName: tenantName,
        TenantReference: tenantReference,
        EnvironmentTag: environmentTag,
        ClientId: clientId,
        ClientName: clientName,
        Scopes: ResolveSeedApiKeyScopes(ResolveEnv("CRONIQ_SEED_API_KEY_SCOPES") ?? string.Empty, logger),
        Overwrite: overwrite);

    await SeedApiKeyEntryAsync(scope.ServiceProvider, singleEntry, logger, token).ConfigureAwait(false);
}

static async Task SeedApiKeyEntryAsync(
    IServiceProvider provider,
    (string KeyId, string Secret, string TenantId, string TenantName, string TenantReference, string EnvironmentTag, string ClientId, string? ClientName, IReadOnlyCollection<string> Scopes, bool Overwrite) entry,
    ILogger logger,
    CancellationToken token)
{
    if (string.IsNullOrWhiteSpace(entry.KeyId) || string.IsNullOrWhiteSpace(entry.Secret))
    {
        logger.LogWarning("API key seeding entry was missing key material; skipping.");
        return;
    }

    var tenantStore = provider.GetRequiredService<ITenantStore>();
    await tenantStore.CreateAsync(new TenantCreateRequest(entry.TenantName, entry.TenantId, entry.TenantReference), token).ConfigureAwait(false);

    if (provider.GetService<SqlServerDbContext>() is SqlServerDbContext sqlContext)
    {
        await SeedSqlServerApiKeyEntryAsync(sqlContext, entry, logger, token).ConfigureAwait(false);
        return;
    }

    if (provider.GetService<PostgresDbContext>() is PostgresDbContext postgresContext)
    {
        await SeedPostgresApiKeyEntryAsync(postgresContext, entry, logger, token).ConfigureAwait(false);
        return;
    }

    throw new InvalidOperationException("No database context was registered for API key seeding.");
}

static async Task SeedSqlServerApiKeyEntryAsync(
    SqlServerDbContext db,
    (string KeyId, string Secret, string TenantId, string TenantName, string TenantReference, string EnvironmentTag, string ClientId, string? ClientName, IReadOnlyCollection<string> Scopes, bool Overwrite) entry,
    ILogger logger,
    CancellationToken token)
{
    var now = DateTime.UtcNow;
    var tenantId = entry.TenantId.Trim();
    var clientId = entry.ClientId.Trim();
    var environmentTag = string.IsNullOrWhiteSpace(entry.EnvironmentTag) ? null : entry.EnvironmentTag.Trim();
    var scopesJson = SerializeScopes(entry.Scopes);

    var client = await db.ApiClients
        .FirstOrDefaultAsync(c => c.TenantId == tenantId && c.ClientId == clientId, token)
        .ConfigureAwait(false);

    if (client is null)
    {
        client = new SqlServerApiClientEntity
        {
            TenantId = tenantId,
            ClientId = clientId,
            Name = entry.ClientName ?? clientId,
            EnvironmentTag = environmentTag,
            ScopesJson = scopesJson,
            IsActive = true,
            IsDeleted = false,
            CreatedAtUtc = now,
            UpdatedAtUtc = now
        };
        db.ApiClients.Add(client);
    }
    else
    {
        client.Name = entry.ClientName ?? client.Name;
        client.EnvironmentTag = environmentTag ?? client.EnvironmentTag;
        client.ScopesJson = scopesJson ?? client.ScopesJson;
        client.IsActive = true;
        client.IsDeleted = false;
        client.UpdatedAtUtc = now;
    }

    var existingKey = await db.ApiKeys
        .Include(k => k.Client)
        .FirstOrDefaultAsync(k => k.KeyId == entry.KeyId, token)
        .ConfigureAwait(false);

    if (existingKey is not null && !entry.Overwrite)
    {
        await db.SaveChangesAsync(token).ConfigureAwait(false);
        logger.LogInformation(
            "API key '{KeyId}' already exists; skipping (set CRONIQ_SEED_API_KEY_OVERWRITE=true to overwrite).",
            entry.KeyId);
        return;
    }

    var salt = GenerateSalt();
    var hash = HashSecret(entry.Secret, salt);

    if (existingKey is null)
    {
        var entity = new SqlServerApiKeyEntity
        {
            Client = client,
            KeyId = entry.KeyId.Trim(),
            SecretSalt = salt,
            SecretHash = hash,
            EnvironmentTag = environmentTag,
            ScopesJson = scopesJson,
            ExpiresAtUtc = null,
            IsActive = true,
            CreatedAtUtc = now,
            UpdatedAtUtc = now
        };
        db.ApiKeys.Add(entity);
    }
    else
    {
        existingKey.Client = client;
        existingKey.SecretSalt = salt;
        existingKey.SecretHash = hash;
        existingKey.EnvironmentTag = environmentTag;
        existingKey.ScopesJson = scopesJson;
        existingKey.IsActive = true;
        existingKey.ExpiresAtUtc = null;
        existingKey.UpdatedAtUtc = now;
    }

    await db.SaveChangesAsync(token).ConfigureAwait(false);
    logger.LogInformation("Seeded API key '{KeyId}' for client '{ClientId}' in tenant '{TenantId}'.", entry.KeyId, clientId, tenantId);
}

static async Task SeedPostgresApiKeyEntryAsync(
    PostgresDbContext db,
    (string KeyId, string Secret, string TenantId, string TenantName, string TenantReference, string EnvironmentTag, string ClientId, string? ClientName, IReadOnlyCollection<string> Scopes, bool Overwrite) entry,
    ILogger logger,
    CancellationToken token)
{
    var now = DateTime.UtcNow;
    var tenantId = entry.TenantId.Trim();
    var clientId = entry.ClientId.Trim();
    var environmentTag = string.IsNullOrWhiteSpace(entry.EnvironmentTag) ? null : entry.EnvironmentTag.Trim();
    var scopesJson = SerializeScopes(entry.Scopes);

    var client = await db.ApiClients
        .FirstOrDefaultAsync(c => c.TenantId == tenantId && c.ClientId == clientId, token)
        .ConfigureAwait(false);

    if (client is null)
    {
        client = new PostgresApiClientEntity
        {
            TenantId = tenantId,
            ClientId = clientId,
            Name = entry.ClientName ?? clientId,
            EnvironmentTag = environmentTag,
            ScopesJson = scopesJson,
            IsActive = true,
            IsDeleted = false,
            CreatedAtUtc = now,
            UpdatedAtUtc = now
        };
        db.ApiClients.Add(client);
    }
    else
    {
        client.Name = entry.ClientName ?? client.Name;
        client.EnvironmentTag = environmentTag ?? client.EnvironmentTag;
        client.ScopesJson = scopesJson ?? client.ScopesJson;
        client.IsActive = true;
        client.IsDeleted = false;
        client.UpdatedAtUtc = now;
    }

    var existingKey = await db.ApiKeys
        .Include(k => k.Client)
        .FirstOrDefaultAsync(k => k.KeyId == entry.KeyId, token)
        .ConfigureAwait(false);

    if (existingKey is not null && !entry.Overwrite)
    {
        await db.SaveChangesAsync(token).ConfigureAwait(false);
        logger.LogInformation(
            "API key '{KeyId}' already exists; skipping (set CRONIQ_SEED_API_KEY_OVERWRITE=true to overwrite).",
            entry.KeyId);
        return;
    }

    var salt = GenerateSalt();
    var hash = HashSecret(entry.Secret, salt);

    if (existingKey is null)
    {
        var entity = new PostgresApiKeyEntity
        {
            Client = client,
            KeyId = entry.KeyId.Trim(),
            SecretSalt = salt,
            SecretHash = hash,
            EnvironmentTag = environmentTag,
            ScopesJson = scopesJson,
            ExpiresAtUtc = null,
            IsActive = true,
            CreatedAtUtc = now,
            UpdatedAtUtc = now
        };
        db.ApiKeys.Add(entity);
    }
    else
    {
        existingKey.Client = client;
        existingKey.SecretSalt = salt;
        existingKey.SecretHash = hash;
        existingKey.EnvironmentTag = environmentTag;
        existingKey.ScopesJson = scopesJson;
        existingKey.IsActive = true;
        existingKey.ExpiresAtUtc = null;
        existingKey.UpdatedAtUtc = now;
    }

    await db.SaveChangesAsync(token).ConfigureAwait(false);
    logger.LogInformation("Seeded API key '{KeyId}' for client '{ClientId}' in tenant '{TenantId}'.", entry.KeyId, clientId, tenantId);
}

static IReadOnlyCollection<string> ResolveSeedAdminScopes(string seedScopesRaw, ILogger logger)
{
    var fallback = new[]
    {
        CroniqScopes.SchedulesWrite,
        CroniqScopes.SchedulesDeadLetter,
        CroniqScopes.CalendarsRead,
        CroniqScopes.CalendarsWrite,
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

static IReadOnlyCollection<string> ResolveSeedApiKeyScopes(string seedScopesRaw, ILogger logger)
{
    var fallback = new[]
    {
        CroniqScopes.WorkPoll,
        CroniqScopes.WorkRenew,
        CroniqScopes.WorkAck,
        CroniqScopes.WorkEvents,
        CroniqScopes.RunnersHeartbeat,
        CroniqScopes.RunnersRead,
        CroniqScopes.ExecutionsRead
    };

    var allKnown = GetAllKnownScopes();

    if (string.IsNullOrWhiteSpace(seedScopesRaw))
    {
        return fallback;
    }

    if (string.Equals(seedScopesRaw, "all", StringComparison.OrdinalIgnoreCase))
    {
        logger.LogInformation("API key seeding: CRONIQ_SEED_API_KEY_SCOPES=all -> granting all scopes ({ScopeCount}).", allKnown.Count);
        return allKnown;
    }

    var parsed = seedScopesRaw
        .Split(new[] { ' ', '\t', '\r', '\n', ',' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .OrderBy(scope => scope, StringComparer.OrdinalIgnoreCase)
        .ToArray();

    if (parsed.Length == 0)
    {
        logger.LogWarning("API key seeding: CRONIQ_SEED_API_KEY_SCOPES was set but empty after parsing; falling back to default scope set.");
        return fallback;
    }

    logger.LogInformation("API key seeding: using custom scope set ({ScopeCount}).", parsed.Length);
    return parsed;
}

static IReadOnlyCollection<(string KeyId, string Secret, string TenantId, string TenantName, string TenantReference, string EnvironmentTag, string ClientId, string? ClientName, IReadOnlyCollection<string> Scopes, bool Overwrite)> ParseSeedApiKeyList(string raw, ILogger logger)
{
    var tenantId = ResolveEnv("CRONIQ_SEED_TENANT_ID")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_ID")
        ?? new CroniqOptions().TenantId.Trim();
    var tenantName = ResolveEnv("CRONIQ_SEED_TENANT_NAME")
        ?? ResolveEnv("CRONIQ_CORE_TENANT_NAME")
        ?? tenantId;
    var tenantReference = ResolveEnv("CRONIQ_SEED_TENANT_REFERENCE")
        ?? tenantId;
    var defaultEnvironment = ResolveEnv("CRONIQ_SEED_API_KEY_ENVIRONMENT")
        ?? ResolveEnv("CRONIQ_ENVIRONMENT")
        ?? new CroniqOptions().EnvironmentTag.Trim();
    var overwrite = string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_API_KEY_OVERWRITE"), "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Environment.GetEnvironmentVariable("CRONIQ_SEED_API_KEY_OVERWRITE"), "1", StringComparison.OrdinalIgnoreCase);

    var entries = new List<(string KeyId, string Secret, string TenantId, string TenantName, string TenantReference, string EnvironmentTag, string ClientId, string? ClientName, IReadOnlyCollection<string> Scopes, bool Overwrite)>();
    foreach (var rawEntry in raw.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
    {
        var parts = rawEntry.Split('|');
        if (parts.Length < 2)
        {
            logger.LogWarning("API key seed entry '{Entry}' is invalid; expected 'keyId.secret|clientId|name|scopes|environment'.", rawEntry);
            continue;
        }

        var (keyId, secret) = SplitSeedKey(parts[0].Trim());
        if (string.IsNullOrWhiteSpace(keyId) || string.IsNullOrWhiteSpace(secret))
        {
            logger.LogWarning("API key seed entry '{Entry}' has invalid key; expected 'keyId.secret'.", rawEntry);
            continue;
        }

        var clientId = parts[1].Trim();
        if (string.IsNullOrWhiteSpace(clientId))
        {
            logger.LogWarning("API key seed entry '{Entry}' missing clientId.", rawEntry);
            continue;
        }

        var name = parts.Length > 2 ? parts[2].Trim() : null;
        var scopesRaw = parts.Length > 3 ? parts[3].Trim() : string.Empty;
        var environment = parts.Length > 4 ? parts[4].Trim() : string.Empty;
        if (string.IsNullOrWhiteSpace(environment))
        {
            environment = defaultEnvironment;
        }

        var scopes = ResolveSeedApiKeyScopes(scopesRaw, logger);
        entries.Add((
            KeyId: keyId,
            Secret: secret,
            TenantId: tenantId,
            TenantName: tenantName,
            TenantReference: tenantReference,
            EnvironmentTag: environment,
            ClientId: clientId,
            ClientName: string.IsNullOrWhiteSpace(name) ? null : name,
            Scopes: scopes,
            Overwrite: overwrite));
    }

    return entries;
}

static (string? KeyId, string? Secret) SplitSeedKey(string seedKey)
{
    var idx = seedKey.IndexOf('.');
    if (idx <= 0 || idx == seedKey.Length - 1)
    {
        return (null, null);
    }

    return (seedKey[..idx], seedKey[(idx + 1)..]);
}

static string GenerateSalt()
{
    Span<byte> buffer = stackalloc byte[16];
    RandomNumberGenerator.Fill(buffer);
    return Convert.ToBase64String(buffer);
}

static string HashSecret(string secret, string salt)
{
    var bytes = Encoding.UTF8.GetBytes(secret + salt);
    var hash = SHA256.HashData(bytes);
    return Convert.ToBase64String(hash);
}

static string? SerializeScopes(IReadOnlyCollection<string> scopes)
{
    if (scopes.Count == 0)
    {
        return null;
    }

    return JsonSerializer.Serialize(scopes, new JsonSerializerOptions(JsonSerializerDefaults.Web));
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
        if (error.Number is 258 or 4060 or 1801 or 18456)
        {
            return true;
        }
    }

    if (exception.InnerException is Win32Exception { NativeErrorCode: 258 })
    {
        return true;
    }

    return false;
}

static bool IsPostgresProvisioningError(Exception exception)
{
    if (exception is TimeoutException)
    {
        return true;
    }

    if (exception is PostgresException postgresException)
    {
        return postgresException.SqlState is "3D000" or "28P01" or "57P03" or "53300" or "08006" or "08001";
    }

    if (exception is NpgsqlException npgsqlException)
    {
        var inner = npgsqlException.InnerException;
        if (inner is not null)
        {
            return IsPostgresProvisioningError(inner);
        }
    }

    if (exception.InnerException is not null)
    {
        return IsPostgresProvisioningError(exception.InnerException);
    }

    return false;
}

static (DatabaseProvider Provider, string ConnectionString) ResolveProvider()
{
    var providerRaw = Environment.GetEnvironmentVariable("CRONIQ_DB_PROVIDER");
    var sqlConnection = Environment.GetEnvironmentVariable("CRONIQ_SQL_CONNECTION");
    var postgresConnection = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_CONNECTION");

    if (!string.IsNullOrWhiteSpace(providerRaw))
    {
        if (string.Equals(providerRaw, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            if (string.IsNullOrWhiteSpace(sqlConnection))
            {
                throw new InvalidOperationException("CRONIQ_SQL_CONNECTION environment variable is required when CRONIQ_DB_PROVIDER=SqlServer.");
            }

            return (DatabaseProvider.SqlServer, sqlConnection);
        }

        if (string.Equals(providerRaw, "Postgres", StringComparison.OrdinalIgnoreCase))
        {
            if (string.IsNullOrWhiteSpace(postgresConnection))
            {
                throw new InvalidOperationException("CRONIQ_POSTGRES_CONNECTION environment variable is required when CRONIQ_DB_PROVIDER=Postgres.");
            }

            return (DatabaseProvider.Postgres, postgresConnection);
        }

        throw new InvalidOperationException("CRONIQ_DB_PROVIDER must be set to 'SqlServer' or 'Postgres'.");
    }

    if (!string.IsNullOrWhiteSpace(postgresConnection) && string.IsNullOrWhiteSpace(sqlConnection))
    {
        return (DatabaseProvider.Postgres, postgresConnection);
    }

    if (!string.IsNullOrWhiteSpace(sqlConnection) && string.IsNullOrWhiteSpace(postgresConnection))
    {
        return (DatabaseProvider.SqlServer, sqlConnection);
    }

    if (!string.IsNullOrWhiteSpace(sqlConnection) && !string.IsNullOrWhiteSpace(postgresConnection))
    {
        throw new InvalidOperationException("Both CRONIQ_SQL_CONNECTION and CRONIQ_POSTGRES_CONNECTION are set. Set CRONIQ_DB_PROVIDER to choose the provider.");
    }

    throw new InvalidOperationException("CRONIQ_SQL_CONNECTION or CRONIQ_POSTGRES_CONNECTION environment variable is required.");
}

enum DatabaseProvider
{
    SqlServer,
    Postgres
}
