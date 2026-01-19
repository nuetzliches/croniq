using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text;
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
    private string? _cliContainerId;
    private string? _cliContainerName;
    private int _cliHostPort;
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

        var dockerTransportDetected = IsDockerTransportAvailable();
        if (!dockerTransportDetected)
        {
            ThrowDockerUnavailableSkip(new InvalidOperationException("Docker engine pipe/socket not detected. Start Docker Desktop or set CRONIQ_POSTGRES."));
        }

        // On Windows, Docker.DotNet/Testcontainers can fail when attaching to container streams
        // ("cannot hijack chunked or content length stream"). Prefer the docker CLI path.
        var forceTestcontainers = string.Equals(
            Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_FORCE_TESTCONTAINERS"),
            "true",
            StringComparison.OrdinalIgnoreCase);

        if (OperatingSystem.IsWindows() && !forceTestcontainers)
        {
            try
            {
                if (await TryStartDockerCliContainerAsync(runtime, throwOnFailure: true).ConfigureAwait(false))
                {
                    return;
                }
            }
            catch (Exception ex)
            {
                ThrowDockerUnavailableSkip(ex);
            }
        }

        try
        {
            await StartTestcontainerAsync(runtime).ConfigureAwait(false);
        }
        catch (Exception ex) when (IsDockerAvailabilityIssue(ex))
        {
            if (dockerTransportDetected && await TryStartDockerCliContainerAsync(runtime, throwOnFailure: false).ConfigureAwait(false))
            {
                return;
            }

            ThrowDockerUnavailableSkip(ex);
        }
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

        if (_cliContainerId is not null)
        {
            LogArtifactPath = await CaptureCliLogsAsync(_cliContainerId, "postgres-contract", CancellationToken.None)
                .ConfigureAwait(false);
            await StopCliContainerAsync().ConfigureAwait(false);
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

        if (_cliContainerId is not null)
        {
            return await CaptureCliLogsAsync(_cliContainerId, artifactName, cancellationToken).ConfigureAwait(false);
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

    private async Task<bool> TryStartDockerCliContainerAsync(PostgresRuntimeOptions runtime, bool throwOnFailure)
    {
        try
        {
            var hostPort = TryGetConfiguredHostPort() ?? GetAvailableTcpPort();
            var containerName = $"croniq-postgres-cli-{Guid.NewGuid():N}";
            var runArgs = new StringBuilder()
                .Append("run -d ")
                .Append("--name ").Append(containerName).Append(' ')
                .Append("-e \"POSTGRES_USER=").Append(runtime.Username).Append("\" ")
                .Append("-e \"POSTGRES_PASSWORD=").Append(runtime.Password).Append("\" ")
                .Append("-e \"POSTGRES_DB=").Append(runtime.DatabaseName).Append("\" ")
                .Append("-p ").Append(hostPort).Append(":5432 ")
                .Append(runtime.Image)
                .ToString();

            var result = await RunDockerCliAsync(runArgs, CancellationToken.None).ConfigureAwait(false);
            if (result.ExitCode != 0)
            {
                var failure = new InvalidOperationException(
                    $"docker run failed (exitCode={result.ExitCode}). stdout='{Truncate(result.StdOut)}' stderr='{Truncate(result.StdErr)}'");
                if (throwOnFailure)
                {
                    throw failure;
                }

                Console.WriteLine($"[PostgresContainerFixture] {failure.Message}");
                return false;
            }

            var containerId = result.StdOut.Trim();
            if (string.IsNullOrWhiteSpace(containerId))
            {
                try
                {
                    await RunDockerCliAsync($"rm -f {containerName}", CancellationToken.None).ConfigureAwait(false);
                }
                catch
                {
                    // ignore cleanup failures
                }

                var failure = new InvalidOperationException(
                    $"docker run did not return a container id. stdout='{Truncate(result.StdOut)}' stderr='{Truncate(result.StdErr)}'");
                if (throwOnFailure)
                {
                    throw failure;
                }

                return false;
            }

            _cliContainerId = containerId;
            _cliContainerName = containerName;
            _cliHostPort = hostPort;

            var connectionString = BuildCliConnectionString(hostPort, runtime);
            Console.WriteLine($"[PostgresContainerFixture] Waiting for docker CLI Postgres container (port {hostPort}) to become ready...");
            await WaitForPostgresAsync(connectionString, ContainerStartupTimeout).ConfigureAwait(false);

            ConnectionString = connectionString;
            await PostgresDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return true;
        }
        catch (Win32Exception)
        {
            if (throwOnFailure)
            {
                throw;
            }

            return false;
        }
        catch (Exception ex)
        {
            var diagnostics = await TryGetCliContainerDiagnosticsAsync(CancellationToken.None).ConfigureAwait(false);
            var failure = new InvalidOperationException(
                $"docker CLI fallback failed. {diagnostics}",
                ex);

            Console.WriteLine($"[PostgresContainerFixture] {failure}");
            await StopCliContainerAsync().ConfigureAwait(false);
            if (throwOnFailure)
            {
                throw failure;
            }

            return false;
        }
    }

    private static string BuildCliConnectionString(int hostPort, PostgresRuntimeOptions runtime)
    {
        var builder = new NpgsqlConnectionStringBuilder
        {
            Host = "127.0.0.1",
            Port = hostPort,
            Database = runtime.DatabaseName,
            Username = runtime.Username,
            Password = runtime.Password,
            Timeout = 15,
            CommandTimeout = 15
        };

        return builder.ConnectionString;
    }

    private static int? TryGetConfiguredHostPort()
    {
        var raw = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_HOST_PORT");
        if (int.TryParse(raw, out var port) && port is > 0 and < 65536)
        {
            return port;
        }

        return null;
    }

    private async Task<string> TryGetCliContainerDiagnosticsAsync(CancellationToken cancellationToken)
    {
        if (_cliContainerId is null)
        {
            return "No CLI container id was recorded.";
        }

        var sb = new StringBuilder();
        try
        {
            var ps = await RunDockerCliAsync($"ps -a --no-trunc --filter id={_cliContainerId}", cancellationToken).ConfigureAwait(false);
            if (ps.ExitCode == 0 && !string.IsNullOrWhiteSpace(ps.StdOut))
            {
                sb.Append("docker ps: ").AppendLine(Truncate(ps.StdOut));
            }
        }
        catch
        {
            // ignore diagnostics failures
        }

        try
        {
            var logs = await RunDockerCliAsync($"logs --tail 200 {_cliContainerId}", cancellationToken).ConfigureAwait(false);
            if (logs.ExitCode == 0 && !string.IsNullOrWhiteSpace(logs.StdOut))
            {
                sb.Append("docker logs (tail): ").AppendLine(Truncate(logs.StdOut));
            }
            else if (logs.ExitCode != 0 && !string.IsNullOrWhiteSpace(logs.StdErr))
            {
                sb.Append("docker logs stderr: ").AppendLine(Truncate(logs.StdErr));
            }
        }
        catch
        {
            // ignore diagnostics failures
        }

        return sb.Length == 0 ? "No docker diagnostics were available." : sb.ToString();
    }

    private static bool IsDockerHijackFailure(Exception exception)
    {
        if (exception.Message.Contains("cannot hijack chunked", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        if (exception.InnerException is not null)
        {
            return IsDockerHijackFailure(exception.InnerException);
        }

        return false;
    }

    private static bool IsDockerAvailabilityIssue(Exception exception)
    {
        if (IsDockerHijackFailure(exception))
        {
            return true;
        }

        if (exception.InnerException is not null)
        {
            return IsDockerAvailabilityIssue(exception.InnerException);
        }

        return exception.Message.Contains("Docker", StringComparison.OrdinalIgnoreCase)
               || exception.Message.Contains("docker", StringComparison.OrdinalIgnoreCase)
               || exception.Message.Contains("daemon", StringComparison.OrdinalIgnoreCase)
               || exception.Message.Contains("npipe", StringComparison.OrdinalIgnoreCase)
               || exception.Message.Contains("socket", StringComparison.OrdinalIgnoreCase);
    }

    private static string Truncate(string? value, int maxLength = 2000)
    {
        if (string.IsNullOrEmpty(value))
        {
            return string.Empty;
        }

        return value.Length <= maxLength ? value : value.Substring(0, maxLength) + "…";
    }

    private static void ThrowDockerUnavailableSkip(Exception exception)
    {
        Console.WriteLine($"[PostgresContainerFixture] Docker unavailable: {exception.Message}");
        throw new InvalidOperationException("Croniq Postgres contract tests require Docker Desktop or a CRONIQ_POSTGRES connection string. Install Docker or set CRONIQ_POSTGRES to reuse an existing database.", exception);
    }

    private async Task StopCliContainerAsync()
    {
        if (_cliContainerId is null)
        {
            return;
        }

        try
        {
            await RunDockerCliAsync($"rm -f {_cliContainerId}", CancellationToken.None).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[PostgresContainerFixture] docker rm failed: {ex.Message}");
        }
        finally
        {
            _cliContainerId = null;
            _cliContainerName = null;
            _cliHostPort = 0;
        }
    }

    private static async Task<string?> CaptureCliLogsAsync(string containerId, string artifactName, CancellationToken cancellationToken)
    {
        var result = await RunDockerCliAsync($"logs {containerId}", cancellationToken).ConfigureAwait(false);
        if (result.ExitCode != 0)
        {
            return null;
        }

        var artifactsDirectory = Path.Combine(Environment.CurrentDirectory, "artifacts");
        Directory.CreateDirectory(artifactsDirectory);
        var filePath = Path.Combine(artifactsDirectory, $"{artifactName}-{DateTimeOffset.UtcNow:yyyyMMddHHmmssfff}.log");
        await File.WriteAllTextAsync(filePath, result.StdOut, cancellationToken).ConfigureAwait(false);
        return filePath;
    }

    private static async Task WaitForPostgresAsync(string connectionString, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            try
            {
                await using var connection = new NpgsqlConnection(connectionString);
                await connection.OpenAsync().ConfigureAwait(false);
                await using var command = connection.CreateCommand();
                command.CommandText = "SELECT 1";
                await command.ExecuteScalarAsync().ConfigureAwait(false);
                return;
            }
            catch (NpgsqlException)
            {
                await Task.Delay(TimeSpan.FromSeconds(2)).ConfigureAwait(false);
            }
            catch (InvalidOperationException)
            {
                await Task.Delay(TimeSpan.FromSeconds(2)).ConfigureAwait(false);
            }
        }

        throw new TimeoutException("Postgres container did not become ready in time.");
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

    private static int GetAvailableTcpPort()
    {
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var port = ((IPEndPoint)listener.LocalEndpoint).Port;
        listener.Stop();
        return port;
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

    private static async Task<(int ExitCode, string StdOut, string StdErr)> RunDockerCliAsync(string arguments, CancellationToken cancellationToken)
    {
        var startInfo = new ProcessStartInfo
        {
            FileName = "docker",
            Arguments = arguments,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true
        };

        using var process = new Process { StartInfo = startInfo };
        if (!process.Start())
        {
            throw new InvalidOperationException("Failed to start docker CLI.");
        }

        var stdoutTask = process.StandardOutput.ReadToEndAsync();
        var stderrTask = process.StandardError.ReadToEndAsync();
        await process.WaitForExitAsync(cancellationToken).ConfigureAwait(false);

        var stdout = await stdoutTask.ConfigureAwait(false);
        var stderr = await stderrTask.ConfigureAwait(false);
        return (process.ExitCode, stdout, stderr);
    }

    private readonly record struct PostgresRuntimeOptions(string Image, string Username, string Password, string DatabaseName);
}
