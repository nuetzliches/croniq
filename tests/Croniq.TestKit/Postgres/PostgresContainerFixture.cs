using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Croniq.TestKit.Diagnostics;
using DotNet.Testcontainers;
using DotNet.Testcontainers.Builders;
using DotNet.Testcontainers.Configurations;
using DotNet.Testcontainers.Containers;
using Npgsql;
using Xunit;

namespace Croniq.TestKit.Postgres;

/// <summary>
/// Provides a shared Postgres instance for contract tests via Testcontainers or an externally supplied connection string.
/// </summary>
public sealed class PostgresContainerFixture : IAsyncLifetime
{
    static PostgresContainerFixture()
    {
        // Disable Ryuk on Windows to avoid Docker.DotNet hijack failures when attaching to the resource reaper stream.
        Environment.SetEnvironmentVariable("TESTCONTAINERS_RYUK_DISABLED", "true");
        TestcontainersSettings.ResourceReaperEnabled = false;
    }

    private PostgreSqlTestcontainer? _container;
    private bool _usingExternal;
    private string? _ownedDatabaseName;
    private string? _ownedDatabaseServerConnectionString;
    private static readonly TimeSpan ContainerStartupTimeout = GetStartupTimeout();

    public string ConnectionString { get; private set; } = string.Empty;

    public bool IsExternal => _usingExternal;

    public string? LogArtifactPath { get; private set; }

    public async Task InitializeAsync()
    {
        LogArtifactPath = null;
        var runtime = CreateRuntimeOptions();
        var external = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES");
        if (!string.IsNullOrWhiteSpace(external))
        {
            _usingExternal = true;
            ConnectionString = await BuildExternalConnectionStringAsync(external, runtime.DatabaseName).ConfigureAwait(false);
            await PostgresDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return;
        }

        if (!IsDockerTransportAvailable())
        {
            ThrowDockerUnavailableSkip(new InvalidOperationException("Docker engine pipe/socket not detected. Start Docker Desktop or set CRONIQ_POSTGRES."));
        }

        await StartTestcontainerAsync(runtime).ConfigureAwait(false);
    }

    private static bool IsDockerTransportAvailable()
    {
        if (OperatingSystem.IsWindows())
        {
            return File.Exists(@"\\.\pipe\docker_engine") || File.Exists(@"\\.\pipe\docker_engine_linux");
        }

        return File.Exists("/var/run/docker.sock");
    }

    public async Task DisposeAsync()
    {
        if (_container is not null)
        {
            LogArtifactPath = await TestcontainerLogCollector
                .CaptureContainerLogsAsync(_container, "postgres-contract", CancellationToken.None)
                .ConfigureAwait(false);

            await _container.DisposeAsync().ConfigureAwait(false);
            _container = null;
            return;
        }

        if (_ownedDatabaseName is not null && _ownedDatabaseServerConnectionString is not null)
        {
            await TryDropOwnedDatabaseAsync(_ownedDatabaseServerConnectionString, _ownedDatabaseName).ConfigureAwait(false);
            _ownedDatabaseName = null;
            _ownedDatabaseServerConnectionString = null;
        }

        LogArtifactPath = null;
    }

    private async Task<string> BuildExternalConnectionStringAsync(string externalConnectionString, string ownedDatabaseName)
    {
        var reuse = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_REUSE_DATABASE");
        var reuseDatabase = string.Equals(reuse, "true", StringComparison.OrdinalIgnoreCase)
            || string.Equals(reuse, "1", StringComparison.OrdinalIgnoreCase);

        var builder = new NpgsqlConnectionStringBuilder(externalConnectionString);
        if (reuseDatabase)
        {
            return builder.ConnectionString;
        }

        var adminBuilder = new NpgsqlConnectionStringBuilder(builder.ConnectionString)
        {
            Database = "postgres"
        };

        try
        {
            await EnsureDatabaseExistsAsync(adminBuilder.ConnectionString, ownedDatabaseName).ConfigureAwait(false);
            _ownedDatabaseName = ownedDatabaseName;
            _ownedDatabaseServerConnectionString = adminBuilder.ConnectionString;

            builder.Database = ownedDatabaseName;
            return builder.ConnectionString;
        }
        catch (Exception ex)
        {
            Console.WriteLine(
                $"[PostgresContainerFixture] External Postgres database isolation failed ({ex.Message}). " +
                "Falling back to the provided CRONIQ_POSTGRES database. " +
                "Set CRONIQ_POSTGRES_REUSE_DATABASE=true to silence this, or grant CREATE DATABASE permissions to enable isolation.");
            return builder.ConnectionString;
        }
    }

    private static async Task EnsureDatabaseExistsAsync(string serverConnectionString, string databaseName)
    {
        await using var connection = new NpgsqlConnection(serverConnectionString);
        await connection.OpenAsync().ConfigureAwait(false);

        await using var check = connection.CreateCommand();
        check.CommandText = "SELECT 1 FROM pg_database WHERE datname = @name;";
        check.Parameters.AddWithValue("name", databaseName);
        var exists = await check.ExecuteScalarAsync().ConfigureAwait(false);
        if (exists is not null && exists is not DBNull)
        {
            return;
        }

        var safeName = databaseName.Replace("\"", "\"\"", StringComparison.Ordinal);
        await using var create = connection.CreateCommand();
        create.CommandText = $"CREATE DATABASE \"{safeName}\";";
        await create.ExecuteNonQueryAsync().ConfigureAwait(false);
    }

    private static async Task TryDropOwnedDatabaseAsync(string serverConnectionString, string databaseName)
    {
        try
        {
            await using var connection = new NpgsqlConnection(serverConnectionString);
            await connection.OpenAsync().ConfigureAwait(false);

            var safeName = databaseName.Replace("\"", "\"\"", StringComparison.Ordinal);
            await using var command = connection.CreateCommand();
            command.CommandText = $"DROP DATABASE IF EXISTS \"{safeName}\";";
            await command.ExecuteNonQueryAsync().ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[PostgresContainerFixture] Failed to drop owned database '{databaseName}': {ex.Message}");
        }
    }

    public Task ResetDatabaseAsync(CancellationToken cancellationToken = default)
    {
        EnsureInitialized();
        return PostgresDatabaseMigrator.ResetDatabaseAsync(ConnectionString, cancellationToken);
    }

    public async Task<string?> CaptureLogsAsync(string artifactName = "postgres", CancellationToken cancellationToken = default)
    {
        if (_container is not null)
        {
            return await TestcontainerLogCollector
                .CaptureContainerLogsAsync(_container, artifactName, cancellationToken)
                .ConfigureAwait(false);
        }

        return null;
    }

    private void EnsureInitialized()
    {
        if (string.IsNullOrWhiteSpace(ConnectionString))
        {
            throw new InvalidOperationException("PostgresContainerFixture has not been initialized yet.");
        }
    }

    private async Task StartTestcontainerAsync(PostgresRuntimeOptions runtime)
    {
        var configuration = new PostgreSqlTestcontainerConfiguration
        {
            Database = runtime.DatabaseName,
            Username = runtime.Username,
            Password = runtime.Password
        };

        _container = new TestcontainersBuilder<PostgreSqlTestcontainer>()
            .WithName($"croniq-postgres-{Guid.NewGuid():N}")
            .WithImage(runtime.Image)
            .WithDatabase(configuration)
            .WithWaitStrategy(Wait.ForUnixContainer().UntilPortIsAvailable(5432))
            .WithCleanUp(true)
            .Build();

        Console.WriteLine($"[PostgresContainerFixture] Starting Postgres container via Testcontainers (image={runtime.Image})...");
        await StartContainerWithTimeoutAsync(_container, ContainerStartupTimeout).ConfigureAwait(false);
        Console.WriteLine("[PostgresContainerFixture] Postgres container started. Applying migrations...");

        ConnectionString = _container.ConnectionString;
        await PostgresDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
    }

    private static void ThrowDockerUnavailableSkip(Exception exception)
    {
        Console.WriteLine($"[PostgresContainerFixture] Docker unavailable: {exception.Message}");
        throw new InvalidOperationException("Croniq Postgres contract tests require Docker Desktop or a CRONIQ_POSTGRES connection string. Install Docker or set CRONIQ_POSTGRES to reuse an existing database.", exception);
    }

    private static async Task StartContainerWithTimeoutAsync(ITestcontainersContainer container, TimeSpan timeout)
    {
        using var cts = new CancellationTokenSource(timeout);
        try
        {
            await container.StartAsync(cts.Token).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            throw new TimeoutException($"Testcontainer start exceeded timeout of {timeout.TotalSeconds} seconds.");
        }
    }

    private static TimeSpan GetStartupTimeout()
    {
        var raw = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_STARTUP_TIMEOUT_SECONDS");
        if (int.TryParse(raw, out var seconds) && seconds > 0)
        {
            return TimeSpan.FromSeconds(seconds);
        }

        return TimeSpan.FromSeconds(120);
    }

    private static PostgresRuntimeOptions CreateRuntimeOptions()
    {
        var username = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_USER") ?? "postgres";
        var password = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_PASSWORD") ?? "postgres";
        var image = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_IMAGE") ?? "postgres:16-alpine";
        var databasePrefix = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_DATABASE") ?? "CroniqTests";
        var database = $"{databasePrefix}_{Environment.ProcessId}_{Guid.NewGuid():N}";
        return new PostgresRuntimeOptions(image, username, password, database);
    }

    private readonly record struct PostgresRuntimeOptions(string Image, string Username, string Password, string DatabaseName);
}
