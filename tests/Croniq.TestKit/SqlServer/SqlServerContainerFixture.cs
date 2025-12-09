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
    private string? _cliContainerId;
    private string? _cliContainerName;
    private int _cliHostPort;

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
            ConnectionString = external;
            await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return;
        }

        try
        {
            await StartTestcontainerAsync(runtime).ConfigureAwait(false);
        }
        catch (InvalidOperationException ex) when (IsDockerHijackFailure(ex))
        {
            if (await TryStartDockerCliContainerAsync(runtime).ConfigureAwait(false))
            {
                return;
            }

            if (await TryInitializeLocalDbFallbackAsync(ex).ConfigureAwait(false))
            {
                return;
            }

            throw;
        }
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

        LogArtifactPath = null;
    }

    public Task ResetDatabaseAsync(CancellationToken cancellationToken = default)
    {
        EnsureInitialized();
        return SqlServerDatabaseMigrator.ResetDatabaseAsync(ConnectionString, cancellationToken);
    }

    public Task<string?> CaptureLogsAsync(string artifactName = "sqlserver", CancellationToken cancellationToken = default)
    {
        if (_container is not null)
        {
            return TestcontainerLogCollector.CaptureContainerLogsAsync(_container, artifactName, cancellationToken);
        }

        if (_cliContainerId is not null)
        {
            return CaptureCliLogsAsync(_cliContainerId, artifactName, cancellationToken);
        }

        return Task.FromResult<string?>(null);
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
            .WithCleanUp(true)
            .Build();

        await _container.StartAsync().ConfigureAwait(false);
        var builder = new SqlConnectionStringBuilder(_container.ConnectionString)
        {
            InitialCatalog = runtime.DatabaseName
        };
        ConnectionString = builder.ConnectionString;

        await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
    }

    private static bool IsDockerHijackFailure(Exception exception)
    {
        return exception.Message.Contains("cannot hijack chunked or content length stream", StringComparison.OrdinalIgnoreCase);
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
            TrustServerCertificate = true,
            ConnectTimeout = 30
        };

        try
        {
            _usingExternal = true;
            ConnectionString = builder.ConnectionString;
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

    private async Task<bool> TryStartDockerCliContainerAsync(SqlServerRuntimeOptions runtime)
    {
        try
        {
            var hostPort = GetAvailableTcpPort();
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
                Console.WriteLine($"[SqlServerContainerFixture] docker run failed: {result.StdErr}");
                return false;
            }

            var containerId = result.StdOut.Trim();
            if (string.IsNullOrWhiteSpace(containerId))
            {
                return false;
            }

            _cliContainerId = containerId;
            _cliContainerName = containerName;
            _cliHostPort = hostPort;

            var connectionString = BuildCliConnectionString(hostPort, runtime);
            var readinessBuilder = new SqlConnectionStringBuilder(connectionString)
            {
                InitialCatalog = "master"
            };

            await WaitForSqlServerAsync(readinessBuilder.ConnectionString, TimeSpan.FromSeconds(90)).ConfigureAwait(false);

            ConnectionString = connectionString;
            await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return true;
        }
        catch (Win32Exception)
        {
            // docker CLI not available on PATH
            return false;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[SqlServerContainerFixture] docker CLI fallback failed: {ex}");
            await StopCliContainerAsync().ConfigureAwait(false);
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
            ConnectTimeout = 30,
            MultipleActiveResultSets = true
        };

        return builder.ConnectionString;
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
        var stdout = new StringBuilder();
        var stderr = new StringBuilder();

        process.OutputDataReceived += (_, e) =>
        {
            if (e.Data is not null)
            {
                stdout.AppendLine(e.Data);
            }
        };

        process.ErrorDataReceived += (_, e) =>
        {
            if (e.Data is not null)
            {
                stderr.AppendLine(e.Data);
            }
        };

        if (!process.Start())
        {
            throw new InvalidOperationException("Failed to start docker CLI.");
        }

        process.BeginOutputReadLine();
        process.BeginErrorReadLine();
        await process.WaitForExitAsync(cancellationToken).ConfigureAwait(false);

        return (process.ExitCode, stdout.ToString(), stderr.ToString());
    }

    private static SqlServerRuntimeOptions CreateRuntimeOptions()
    {
        var password = Environment.GetEnvironmentVariable("CRONIQ_SQL_PASSWORD") ?? "YourStrong(!)Password1";
        var image = Environment.GetEnvironmentVariable("CRONIQ_SQL_IMAGE") ?? "mcr.microsoft.com/mssql/server:2022-latest";
        var database = Environment.GetEnvironmentVariable("CRONIQ_SQL_DATABASE") ?? $"CroniqTests_{Environment.ProcessId}_{Guid.NewGuid():N}";
        var pid = Environment.GetEnvironmentVariable("CRONIQ_SQL_PID") ?? "Developer";
        return new SqlServerRuntimeOptions(image, password, database, pid);
    }

    private readonly record struct SqlServerRuntimeOptions(string Image, string Password, string DatabaseName, string SqlPid);
}
