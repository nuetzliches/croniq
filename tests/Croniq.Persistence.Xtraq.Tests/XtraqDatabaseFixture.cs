using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using Microsoft.Data.SqlClient;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Croniq.Persistence.Xtraq.Tests;

/// <summary>
/// Boots a local Xtraq database (via infra/sql/xtraq/apply.ps1) and exposes a configured provider factory.
/// Skips the suite when CRONIQ_SQL is not set.
/// </summary>
public sealed class XtraqDatabaseFixture : IAsyncLifetime
{
    public const string DefaultInstanceId = "integration-instance";

    private string? _skipReason;
    private SqlConnectionStringBuilder? _builder;

    public string? ConnectionString { get; private set; }
    public string? SkipReason => _skipReason;

    public async Task InitializeAsync()
    {
        var cs = Environment.GetEnvironmentVariable("CRONIQ_SQL");
        if (string.IsNullOrWhiteSpace(cs))
        {
            _skipReason = "CRONIQ_SQL is not set; integration tests are skipped.";
            return;
        }

        ConnectionString = cs;
        _builder = new SqlConnectionStringBuilder(cs);

        var applyScript = LocateApplyScript();
        if (!File.Exists(applyScript))
        {
            _skipReason = $"apply.ps1 not found at {applyScript}";
            return;
        }

        await EnsureDatabaseAsync(applyScript).ConfigureAwait(false);
        await EnsureDefaultTenantAsync().ConfigureAwait(false);
        await EnsureDefaultInstanceAsync().ConfigureAwait(false);
    }

    public Task DisposeAsync() => Task.CompletedTask;

    public IServiceProvider CreateProvider()
    {
        if (_skipReason is not null) throw new InvalidOperationException(_skipReason);
        if (ConnectionString is null) throw new InvalidOperationException("Fixture not initialized.");

        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqXtraqPersistence(opts =>
        {
            opts.ConnectionString = ConnectionString;
            opts.Actor = "integration-test";
        });

        return services.BuildServiceProvider();
    }

    private static string LocateApplyScript()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir != null)
        {
            var candidate = Path.Combine(dir.FullName, "infra", "sql", "xtraq", "apply.ps1");
            if (File.Exists(candidate))
            {
                return candidate;
            }

            dir = dir.Parent;
        }

        throw new InvalidOperationException("Could not locate infra/sql/xtraq/apply.ps1.");
    }

    private async Task EnsureDatabaseAsync(string applyScript)
    {
        if (_builder is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        var args = new List<string>
        {
            "-NoProfile",
            "-ExecutionPolicy", "Bypass",
            "-File", $"\"{applyScript}\"",
            "-Server", $"\"{_builder.DataSource}\"",
            "-Database", $"\"{_builder.InitialCatalog}\""
        };

        if (_builder.TrustServerCertificate)
        {
            args.Add("-TrustServerCertificate");
        }

        if (_builder.IntegratedSecurity)
        {
            args.Add("-User");
            args.Add("\"\"");
        }
        else if (!string.IsNullOrWhiteSpace(_builder.UserID))
        {
            args.Add("-User");
            args.Add($"\"{_builder.UserID}\"");
        }

        if (!_builder.IntegratedSecurity && !string.IsNullOrWhiteSpace(_builder.Password))
        {
            args.Add("-Password");
            args.Add($"\"{_builder.Password}\"");
        }

        var startInfo = new ProcessStartInfo
        {
            FileName = "powershell",
            Arguments = string.Join(" ", args),
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false
        };

        using var process = new Process { StartInfo = startInfo };
        process.Start();
        var stdout = await process.StandardOutput.ReadToEndAsync().ConfigureAwait(false);
        var stderr = await process.StandardError.ReadToEndAsync().ConfigureAwait(false);
        await process.WaitForExitAsync().ConfigureAwait(false);

        if (process.ExitCode != 0)
        {
            throw new InvalidOperationException($"apply.ps1 failed with exit code {process.ExitCode}: {stderr}{stdout}");
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
