using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.TestKit.Diagnostics;
using DotNet.Testcontainers.Builders;
using DotNet.Testcontainers.Configurations;
using DotNet.Testcontainers.Containers;
using Xunit;

namespace Croniq.TestKit.SqlServer;

/// <summary>
/// Provides a shared SQL Server instance for contract tests via Testcontainers or an externally supplied connection string.
/// </summary>
public sealed class SqlServerContainerFixture : IAsyncLifetime
{
    private MsSqlTestcontainer? _container;
    private bool _usingExternal;

    public string ConnectionString { get; private set; } = string.Empty;

    public bool IsExternal => _usingExternal;

    public string? LogArtifactPath { get; private set; }

    public async Task InitializeAsync()
    {
        LogArtifactPath = null;
        var external = Environment.GetEnvironmentVariable("CRONIQ_SQL");
        if (!string.IsNullOrWhiteSpace(external))
        {
            _usingExternal = true;
            ConnectionString = external;
            await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
            return;
        }

        var image = Environment.GetEnvironmentVariable("CRONIQ_SQL_IMAGE") ?? "mcr.microsoft.com/mssql/server:2022-latest";
        var configuration = new MsSqlTestcontainerConfiguration
        {
            Password = Environment.GetEnvironmentVariable("CRONIQ_SQL_PASSWORD") ?? "YourStrong(!)Password1"
        };

        _container = new TestcontainersBuilder<MsSqlTestcontainer>()
            .WithName($"croniq-sql-{Guid.NewGuid():N}")
            .WithImage(image)
            .WithDatabase(configuration)
            .WithCleanUp(true)
            .Build();

        await _container.StartAsync().ConfigureAwait(false);
        ConnectionString = _container.ConnectionString;

        await SqlServerDatabaseMigrator.ApplyMigrationsAsync(ConnectionString).ConfigureAwait(false);
    }

    public async Task DisposeAsync()
    {
        if (_container is null)
        {
            LogArtifactPath = null;
            return;
        }

        LogArtifactPath = await TestcontainerLogCollector
            .CaptureContainerLogsAsync(_container, "sqlserver-contract", CancellationToken.None)
            .ConfigureAwait(false);

        await _container.DisposeAsync().ConfigureAwait(false);
        _container = null;
    }

    public Task ResetDatabaseAsync(CancellationToken cancellationToken = default)
    {
        EnsureInitialized();
        return SqlServerDatabaseMigrator.ResetDatabaseAsync(ConnectionString, cancellationToken);
    }

    public Task<string?> CaptureLogsAsync(string artifactName = "sqlserver", CancellationToken cancellationToken = default)
    {
        if (_container is null)
        {
            return Task.FromResult<string?>(null);
        }

        return TestcontainerLogCollector.CaptureContainerLogsAsync(_container, artifactName, cancellationToken);
    }

    private void EnsureInitialized()
    {
        if (string.IsNullOrWhiteSpace(ConnectionString))
        {
            throw new InvalidOperationException("SqlServerContainerFixture has not been initialized yet.");
        }
    }
}
