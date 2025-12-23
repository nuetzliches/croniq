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
using Microsoft.Data.SqlClient;
using Xunit;

namespace Croniq.TestKit.SqlServer;

/// <summary>
/// Provides a shared SQL Server instance for contract tests via Testcontainers or an externally supplied connection string.
/// </summary>
public sealed class SqlServerContainerFixture : IAsyncLifetime
{
    static SqlServerContainerFixture()
    {
        // Disable Ryuk on Windows to avoid Docker.DotNet hijack failures when attaching to the resource reaper stream.
        Environment.SetEnvironmentVariable("TESTCONTAINERS_RYUK_DISABLED", "true");
        TestcontainersSettings.ResourceReaperEnabled = false;
    }

    private MsSqlTestcontainer? _container;
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
        var external = Environment.GetEnvironmentVariable("CRONIQ_SQL");
        if (!string.IsNullOrWhiteSpace(external))
        {
            _usingExternal = true;
            ConnectionString = await BuildExternalConnectionStringAsync(external, runtime.DatabaseName).ConfigureAwait(false);
            await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return;
        }

        var dockerTransportDetected = IsDockerTransportAvailable();
        if (!dockerTransportDetected)
        {
            var dockerMissing = new InvalidOperationException("Docker engine pipe/socket not detected. Start Docker Desktop or set CRONIQ_SQL.");
            Console.WriteLine("[SqlServerContainerFixture] Docker pipe/socket not found. Falling back to LocalDB/CRONIQ_SQL overrides.");
            if (await TryInitializeLocalDbFallbackAsync(dockerMissing).ConfigureAwait(false))
            {
                return;
            }

            ThrowDockerUnavailableSkip(dockerMissing);
        }

        // On Windows, Docker.DotNet/Testcontainers can fail when attaching to container streams
        // ("cannot hijack chunked or content length stream"). Prefer the docker CLI path.
        var forceTestcontainers = string.Equals(
            Environment.GetEnvironmentVariable("CRONIQ_SQL_FORCE_TESTCONTAINERS"),
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
                if (await TryInitializeLocalDbFallbackAsync(ex).ConfigureAwait(false))
                {
                    return;
                }

                ThrowDockerUnavailableSkip(ex);
            }
        }

        try
        {
            await StartTestcontainerAsync(runtime).ConfigureAwait(false);
        }
        catch (Exception ex) when (IsDockerAvailabilityIssue(ex))
        {
            Console.WriteLine($"[SqlServerContainerFixture] Testcontainers start failed: {ex.Message}");
            if (dockerTransportDetected && await TryStartDockerCliContainerAsync(runtime, throwOnFailure: false).ConfigureAwait(false))
            {
                return;
            }

            if (await TryInitializeLocalDbFallbackAsync(ex).ConfigureAwait(false))
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
                .CaptureContainerLogsAsync(_container, "sqlserver-contract", CancellationToken.None)
                .ConfigureAwait(false);

            await _container.DisposeAsync().ConfigureAwait(false);
            _container = null;
            return;
        }

        if (_cliContainerId is not null)
        {
            LogArtifactPath = await CaptureCliLogsAsync(_cliContainerId, "sqlserver-contract", CancellationToken.None)
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
        var reuse = Environment.GetEnvironmentVariable("CRONIQ_SQL_REUSE_DATABASE");
        var reuseDatabase = string.Equals(reuse, "true", StringComparison.OrdinalIgnoreCase)
            || string.Equals(reuse, "1", StringComparison.OrdinalIgnoreCase);

        var builder = new SqlConnectionStringBuilder(externalConnectionString)
        {
            Encrypt = false,
            TrustServerCertificate = true,
            MultipleActiveResultSets = false
        };

        if (reuseDatabase)
        {
            return builder.ConnectionString;
        }

        var masterBuilder = new SqlConnectionStringBuilder(builder.ConnectionString)
        {
            InitialCatalog = "master",
            ConnectTimeout = Math.Max(builder.ConnectTimeout, 15)
        };

        try
        {
            await EnsureDatabaseExistsAsync(masterBuilder.ConnectionString, ownedDatabaseName).ConfigureAwait(false);
            _ownedDatabaseName = ownedDatabaseName;
            _ownedDatabaseServerConnectionString = masterBuilder.ConnectionString;

            builder.InitialCatalog = ownedDatabaseName;
            return builder.ConnectionString;
        }
        catch (Exception ex)
        {
            Console.WriteLine(
                $"[SqlServerContainerFixture] External SQL database isolation failed ({ex.Message}). " +
                "Falling back to the provided CRONIQ_SQL database. " +
                "Set CRONIQ_SQL_REUSE_DATABASE=true to silence this, or grant CREATE DATABASE permissions to enable isolation.");
            return builder.ConnectionString;
        }
    }

    private static async Task EnsureDatabaseExistsAsync(string serverConnectionString, string databaseName)
    {
        var safeName = databaseName.Replace("]", "]]", StringComparison.Ordinal);
        var sql = $"IF DB_ID(N'{databaseName.Replace("'", "''", StringComparison.Ordinal)}') IS NULL CREATE DATABASE [{safeName}];";

        await using var connection = new SqlConnection(serverConnectionString);
        await connection.OpenAsync().ConfigureAwait(false);
        await using var command = connection.CreateCommand();
        command.CommandText = sql;
        await command.ExecuteNonQueryAsync().ConfigureAwait(false);
    }

    private static async Task TryDropOwnedDatabaseAsync(string serverConnectionString, string databaseName)
    {
        try
        {
            var safeName = databaseName.Replace("]", "]]", StringComparison.Ordinal);
            var escaped = databaseName.Replace("'", "''", StringComparison.Ordinal);
            var sql = $@"IF DB_ID(N'{escaped}') IS NOT NULL
BEGIN
    ALTER DATABASE [{safeName}] SET SINGLE_USER WITH ROLLBACK IMMEDIATE;
    DROP DATABASE [{safeName}];
END";

            await using var connection = new SqlConnection(serverConnectionString);
            await connection.OpenAsync().ConfigureAwait(false);
            await using var command = connection.CreateCommand();
            command.CommandText = sql;
            await command.ExecuteNonQueryAsync().ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[SqlServerContainerFixture] Failed to drop owned database '{databaseName}': {ex.Message}");
        }
    }

    public Task ResetDatabaseAsync(CancellationToken cancellationToken = default)
    {
        EnsureInitialized();
        return SqlServerDatabaseMigrator.ResetDatabaseAsync(ConnectionString, cancellationToken);
    }

    public async Task<string?> CaptureLogsAsync(string artifactName = "sqlserver", CancellationToken cancellationToken = default)
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
            throw new InvalidOperationException("SqlServerContainerFixture has not been initialized yet.");
        }
    }

    private async Task StartTestcontainerAsync(SqlServerRuntimeOptions runtime)
    {
        var configuration = new MsSqlTestcontainerConfiguration
        {
            Password = runtime.Password
        };

        _container = new TestcontainersBuilder<MsSqlTestcontainer>()
            .WithName($"croniq-sql-{Guid.NewGuid():N}")
            .WithImage(runtime.Image)
            .WithDatabase(configuration)
            .WithEnvironment("ACCEPT_EULA", "Y")
            .WithEnvironment("MSSQL_PID", runtime.SqlPid)
            // The MsSqlTestcontainer default wait strategy may attach to logs. On some Windows Docker setups,
            // Docker.DotNet fails to hijack the log stream. Waiting for port availability avoids that attach.
            .WithWaitStrategy(Wait.ForUnixContainer().UntilPortIsAvailable(1433))
            .WithCleanUp(true)
            .Build();

        Console.WriteLine($"[SqlServerContainerFixture] Starting SQL container via Testcontainers (image={runtime.Image})...");
        await StartContainerWithTimeoutAsync(_container, ContainerStartupTimeout).ConfigureAwait(false);
        Console.WriteLine("[SqlServerContainerFixture] SQL container started. Waiting for readiness...");
        var builder = new SqlConnectionStringBuilder(_container.ConnectionString)
        {
            InitialCatalog = runtime.DatabaseName,
            Encrypt = false,
            TrustServerCertificate = true
        };
        ConnectionString = builder.ConnectionString;

        await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
    }

    private static bool IsDockerHijackFailure(Exception exception)
    {
        return exception.Message.Contains("cannot hijack chunked or content length stream", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsDockerAvailabilityIssue(Exception exception)
    {
        if (exception is TimeoutException || exception is Win32Exception || exception is SocketException)
        {
            return true;
        }

        if (exception is AggregateException aggregate)
        {
            foreach (var inner in aggregate.InnerExceptions)
            {
                if (IsDockerAvailabilityIssue(inner))
                {
                    return true;
                }
            }
        }

        if (IsDockerHijackFailure(exception))
        {
            return true;
        }

        if (exception.InnerException is not null)
        {
            return IsDockerAvailabilityIssue(exception.InnerException);
        }

        return exception.Message.Contains("Docker", StringComparison.OrdinalIgnoreCase)
               || exception.Message.Contains("named pipe", StringComparison.OrdinalIgnoreCase)
               || exception.Message.Contains("pipe busy", StringComparison.OrdinalIgnoreCase);
    }

    private static void ThrowDockerUnavailableSkip(Exception exception)
    {
        Console.WriteLine($"[SqlServerContainerFixture] Docker unavailable: {exception.Message}");
        throw new InvalidOperationException("Croniq SQL Server contract tests require Docker Desktop, LocalDB, or a CRONIQ_SQL connection string. Install Docker or set CRONIQ_SQL to reuse an existing database.", exception);
    }

    private async Task<bool> TryInitializeLocalDbFallbackAsync(Exception dockerException)
    {
        if (!OperatingSystem.IsWindows())
        {
            return false;
        }

        var builder = new SqlConnectionStringBuilder
        {
            DataSource = @"(localdb)\\MSSQLLocalDB",
            InitialCatalog = $"CroniqTests_{Environment.ProcessId}_{Guid.NewGuid():N}",
            IntegratedSecurity = true,
            Encrypt = false,
            TrustServerCertificate = true,
            ConnectTimeout = 30
        };

        try
        {
            _usingExternal = true;
            ConnectionString = builder.ConnectionString;
            _ownedDatabaseName = builder.InitialCatalog;
            var serverBuilder = new SqlConnectionStringBuilder(builder.ConnectionString)
            {
                InitialCatalog = "master"
            };
            _ownedDatabaseServerConnectionString = serverBuilder.ConnectionString;
            await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return true;
        }
        catch (Exception fallbackException)
        {
            throw new InvalidOperationException(
                "Docker-based SQL Server start failed and the LocalDB fallback was not available. Configure CRONIQ_SQL to point to an existing database.",
                new AggregateException(dockerException, fallbackException));
        }
    }

    private async Task<bool> TryStartDockerCliContainerAsync(SqlServerRuntimeOptions runtime, bool throwOnFailure)
    {
        try
        {
            var hostPort = TryGetConfiguredHostPort() ?? GetAvailableTcpPort();
            var containerName = $"croniq-sql-cli-{Guid.NewGuid():N}";
            var runArgs = new StringBuilder()
                .Append("run -d ")
                .Append("--name ").Append(containerName).Append(' ')
                .Append("-e ACCEPT_EULA=Y ")
                .Append("-e \"MSSQL_SA_PASSWORD=").Append(runtime.Password).Append("\" ")
                .Append("-e \"MSSQL_PID=").Append(runtime.SqlPid).Append("\" ")
                .Append("-p ").Append(hostPort).Append(":1433 ")
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

                Console.WriteLine($"[SqlServerContainerFixture] {failure.Message}");
                return false;
            }

            var containerId = result.StdOut.Trim();
            if (string.IsNullOrWhiteSpace(containerId))
            {
                // If stdout capture failed, the container might still have been created. Clean up by name.
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
            var readinessBuilder = new SqlConnectionStringBuilder(connectionString)
            {
                InitialCatalog = "master",
                ConnectTimeout = 5,
                Encrypt = false,
                TrustServerCertificate = true
            };

            Console.WriteLine($"[SqlServerContainerFixture] Waiting for docker CLI SQL container (port {hostPort}) to become ready...");
            await WaitForSqlServerAsync(readinessBuilder.ConnectionString, ContainerStartupTimeout).ConfigureAwait(false);

            ConnectionString = connectionString;
            await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return true;
        }
        catch (Win32Exception)
        {
            // docker CLI not available on PATH
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

            Console.WriteLine($"[SqlServerContainerFixture] {failure}");
            await StopCliContainerAsync().ConfigureAwait(false);
            if (throwOnFailure)
            {
                throw failure;
            }

            return false;
        }
    }

    private static string BuildCliConnectionString(int hostPort, SqlServerRuntimeOptions runtime)
    {
        var builder = new SqlConnectionStringBuilder
        {
            DataSource = $"127.0.0.1,{hostPort}",
            InitialCatalog = runtime.DatabaseName,
            UserID = "sa",
            Password = runtime.Password,
            TrustServerCertificate = true,
            Encrypt = false,
            ConnectTimeout = 15,
            MultipleActiveResultSets = false
        };

        return builder.ConnectionString;
    }

    private static int? TryGetConfiguredHostPort()
    {
        var raw = Environment.GetEnvironmentVariable("CRONIQ_SQL_HOST_PORT");
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

    private static string Truncate(string? value, int maxLength = 2000)
    {
        if (string.IsNullOrEmpty(value))
        {
            return string.Empty;
        }

        return value.Length <= maxLength ? value : value.Substring(0, maxLength) + "…";
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
            Console.WriteLine($"[SqlServerContainerFixture] docker rm failed: {ex.Message}");
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

    private static async Task WaitForSqlServerAsync(string connectionString, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            try
            {
                await using var connection = new SqlConnection(connectionString);
                await connection.OpenAsync().ConfigureAwait(false);
                return;
            }
            catch (SqlException)
            {
                await Task.Delay(TimeSpan.FromSeconds(2)).ConfigureAwait(false);
            }
            catch (InvalidOperationException)
            {
                await Task.Delay(TimeSpan.FromSeconds(2)).ConfigureAwait(false);
            }
        }

        throw new TimeoutException("SQL Server container did not become ready in time.");
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
        // The first SQL Server container start can be slow on Windows (image pull, volume init).
        // Keep this configurable for CI and local troubleshooting.
        var raw = Environment.GetEnvironmentVariable("CRONIQ_SQL_STARTUP_TIMEOUT_SECONDS");
        if (int.TryParse(raw, out var seconds) && seconds > 0)
        {
            return TimeSpan.FromSeconds(seconds);
        }

        return TimeSpan.FromSeconds(180);
    }

    private static int GetAvailableTcpPort()
    {
        var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var port = ((IPEndPoint)listener.LocalEndpoint).Port;
        listener.Stop();
        return port;
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

    private static SqlServerRuntimeOptions CreateRuntimeOptions()
    {
        var password = Environment.GetEnvironmentVariable("CRONIQ_SQL_PASSWORD") ?? "YourStrong(!)Password1";
        var image = Environment.GetEnvironmentVariable("CRONIQ_SQL_IMAGE") ?? "mcr.microsoft.com/mssql/server:2022-latest";
        var databasePrefix = Environment.GetEnvironmentVariable("CRONIQ_SQL_DATABASE") ?? "CroniqTests";
        // Always isolate contract tests from dev/prod DB names like "CroniqDev".
        // Even if a prefix is provided, append process + GUID for uniqueness.
        var database = $"{databasePrefix}_{Environment.ProcessId}_{Guid.NewGuid():N}";
        var pid = Environment.GetEnvironmentVariable("CRONIQ_SQL_PID") ?? "Developer";
        return new SqlServerRuntimeOptions(image, password, database, pid);
    }

    private readonly record struct SqlServerRuntimeOptions(string Image, string Password, string DatabaseName, string SqlPid);
}
