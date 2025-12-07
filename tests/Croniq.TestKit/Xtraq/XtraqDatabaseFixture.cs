using Croniq.Persistence.Xtraq;
using Croniq.TestKit.Containers;
using Croniq.TestKit.Sql;
using DotNet.Testcontainers.Builders;
using DotNet.Testcontainers.Configurations;
using DotNet.Testcontainers.Containers;
using Microsoft.Data.SqlClient;
using Microsoft.Extensions.DependencyInjection;
using System.IO;
using System.Threading;
using Xunit;

namespace Croniq.TestKit;

/// <summary>
/// Applies the Xtraq schema to the database referenced by the CRONIQ_SQL environment variable.
/// </summary>
public sealed class XtraqDatabaseFixture : IAsyncLifetime
{
    public const string DefaultInstanceId = "integration-instance";
    private const string DefaultDatabaseName = "CroniqIntegration";
    private const string DefaultPassword = "StrongP@ssw0rd!";
    private const string SqlServerImage = "mcr.microsoft.com/mssql/server:2022-latest";

    private static readonly string[] ScriptOrder =
    [
        "predeploy.sql",
        "core/types.sql",
        "core/procs.health.sql",
        "core-internal/types.sql",
        "core-internal/procs.errors.sql",
        "core-internal/procs.guards.sql",
        "croniq/types.sql",
        "croniq/functions.sql",
        "croniq-internal/types.sql",
        "croniq-internal/procs.errors.sql",
        "croniq-internal/procs.guards.sql",
        "auth/types.sql",
        "auth/tables.sql",
        "auth/procs.keys.sql",
        "croniq/tables.sql",
        "croniq/procs.instances.sql",
        "croniq/procs.jobs.sql",
        "croniq/procs.leases.sql",
        "croniq/procs.deadletter.sql",
        "seed.dev.sql"
    ];

    private SqlConnectionStringBuilder? _builder;
    private string? _skipReason;
    private MsSqlTestcontainer? _container;
    private MemoryStream? _logBuffer;

    public string? ConnectionString { get; private set; }
    public string? SkipReason => _skipReason;
    public bool IsEphemeralDatabase => _container is not null;

    public async Task InitializeAsync()
    {
        var connectionString = Environment.GetEnvironmentVariable("CRONIQ_SQL");
        if (string.IsNullOrWhiteSpace(connectionString))
        {
            try
            {
                _logBuffer = new MemoryStream();
                var logConsumer = Consume.RedirectStdoutAndStderrToStream(_logBuffer, _logBuffer);

                _container = new TestcontainersBuilder<MsSqlTestcontainer>()
                    .WithDatabase(new MsSqlTestcontainerConfiguration
                    {
                        Database = DefaultDatabaseName,
                        Password = DefaultPassword
                    })
                    .WithImage(SqlServerImage)
                    .WithName($"croniq-sql-{Guid.NewGuid():N}")
                    .WithOutputConsumer(logConsumer)
                    .Build();

                await _container.StartAsync().ConfigureAwait(false);
                connectionString = _container.ConnectionString;
            }
            catch (Exception ex)
            {
                _skipReason = $"Failed to start SQL Server container: {ex.Message}";
                return;
            }
        }

        ConnectionString = connectionString;
        _builder = new SqlConnectionStringBuilder(connectionString);

        if (string.IsNullOrWhiteSpace(_builder.InitialCatalog))
        {
            _builder.InitialCatalog = DefaultDatabaseName;
            ConnectionString = _builder.ConnectionString;
        }

        try
        {
            await EnsureDatabaseAsync().ConfigureAwait(false);
            await ApplySchemaAsync().ConfigureAwait(false);
            await EnsureDefaultTenantAsync().ConfigureAwait(false);
            await EnsureDefaultInstanceAsync().ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _skipReason = $"Failed to initialize Xtraq schema: {ex.Message}";
        }
    }

    public async Task DisposeAsync()
    {
        if (_container is not null)
        {
            await _container.DisposeAsync().ConfigureAwait(false);
        }

        _logBuffer?.Dispose();
    }

    public Task<string?> CaptureContainerLogsAsync(string outputDirectory, CancellationToken cancellationToken = default)
    {
        return TestcontainerLogCollector.TryWriteLogsAsync(_logBuffer, outputDirectory, "xtraq-sql", cancellationToken);
    }

    public IServiceProvider CreateProvider()
    {
        if (_skipReason is not null)
        {
            throw new InvalidOperationException(_skipReason);
        }

        if (ConnectionString is null)
        {
            throw new InvalidOperationException("Fixture not initialized.");
        }

        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqXtraqPersistence(options =>
        {
            options.ConnectionString = ConnectionString;
            options.Actor = "integration-test";
        });

        return services.BuildServiceProvider();
    }

    private async Task EnsureDatabaseAsync()
    {
        if (_builder is null)
        {
            throw new InvalidOperationException("Connection builder is not configured.");
        }

        var database = _builder.InitialCatalog;
        if (string.IsNullOrWhiteSpace(database))
        {
            throw new InvalidOperationException("Database name is not configured.");
        }
        var masterBuilder = new SqlConnectionStringBuilder(_builder.ConnectionString)
        {
            InitialCatalog = "master"
        };

        await using var connection = new SqlConnection(masterBuilder.ConnectionString);
        await connection.OpenAsync().ConfigureAwait(false);
        await using var command = connection.CreateCommand();
        command.CommandText = $"IF DB_ID('{database}') IS NULL CREATE DATABASE [{database}];";
        await command.ExecuteNonQueryAsync().ConfigureAwait(false);
    }

    private async Task ApplySchemaAsync()
    {
        if (ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        var scriptsRoot = RepositoryPaths.GetXtraqSqlRoot();
        await using var connection = new SqlConnection(ConnectionString);
        await connection.OpenAsync().ConfigureAwait(false);

        foreach (var relative in ScriptOrder)
        {
            var fullPath = Path.Combine(scriptsRoot, relative.Replace('/', Path.DirectorySeparatorChar));
            await SqlScriptBatchExecutor.ExecuteAsync(connection, fullPath, CancellationToken.None).ConfigureAwait(false);
        }
    }

    private async Task EnsureDefaultTenantAsync()
    {
        if (ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(ConnectionString);
        await conn.OpenAsync().ConfigureAwait(false);
        await using var cmd = new SqlCommand("""
            IF NOT EXISTS (SELECT 1 FROM auth.Tenants WHERE TenantId = 1)
            BEGIN
                SET IDENTITY_INSERT auth.Tenants ON;
                INSERT INTO auth.Tenants (TenantId, Reference, Name, CreatedBy, IsDeleted)
                VALUES (1, 'tenant-1', 'default', 'integration-test', 0);
                SET IDENTITY_INSERT auth.Tenants OFF;
            END
            """, conn);
        await cmd.ExecuteNonQueryAsync().ConfigureAwait(false);
    }

    private async Task EnsureDefaultInstanceAsync()
    {
        if (ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(ConnectionString);
        await conn.OpenAsync().ConfigureAwait(false);
        await using var cmd = new SqlCommand("""
            IF NOT EXISTS (SELECT 1 FROM croniq.Instances WHERE InstanceId = @id)
            BEGIN
                INSERT INTO croniq.Instances (InstanceId, Environment, NodeName, Capabilities, Version, CreatedBy, IsDeleted)
                VALUES (@id, 'dev', 'integration-node', NULL, 'test', 'integration-test', 0);
            END
            """, conn);
        cmd.Parameters.AddWithValue("@id", DefaultInstanceId);
        await cmd.ExecuteNonQueryAsync().ConfigureAwait(false);
    }
}
