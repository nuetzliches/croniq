using System.Globalization;

using Croniq.Runner.Sdk.Configuration;

using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Resolves the runner ID with the same precedence as the Rust SDK's
/// <c>identity.rs</c>:
/// <list type="number">
///   <item><description>explicit <see cref="CroniqRunnerOptions.RunnerId"/> from configuration</description></item>
///   <item><description><c>RUNNER_ID</c> environment variable</description></item>
///   <item><description>persisted state file under <see cref="CroniqRunnerOptions.RunnerDataDir"/> (or <c>CRONIQ_RUNNER_DATA_DIR</c>, or the platform default)</description></item>
///   <item><description>newly generated <c>{prefix}-{guid8}</c>, persisted to the state file</description></item>
/// </list>
/// </summary>
internal sealed class RunnerIdentityResolver(
    IOptions<CroniqRunnerOptions> options,
    ILogger<RunnerIdentityResolver> logger)
{
    private readonly CroniqRunnerOptions _options = options.Value;
    private readonly ILogger _logger = logger;

    public string Resolve()
    {
        if (!string.IsNullOrEmpty(_options.RunnerId))
        {
            return _options.RunnerId;
        }

        var envId = Environment.GetEnvironmentVariable("RUNNER_ID");
        if (!string.IsNullOrEmpty(envId))
        {
            return envId;
        }

        var dataDir = ResolveDataDir();
        var idFile = Path.Combine(dataDir, "runner-id");
        try
        {
            if (File.Exists(idFile))
            {
                var persisted = File.ReadAllText(idFile).Trim();
                if (!string.IsNullOrEmpty(persisted))
                {
                    return persisted;
                }
            }
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "could not read persisted runner ID from {Path}", idFile);
        }

        var generated = GenerateId(_options.RunnerIdPrefix);
        try
        {
            Directory.CreateDirectory(dataDir);
            File.WriteAllText(idFile, generated);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "could not persist generated runner ID to {Path}", idFile);
            return $"{_options.RunnerIdPrefix}-{Environment.MachineName.ToLowerInvariant()}";
        }

        return generated;
    }

    private string ResolveDataDir()
    {
        if (!string.IsNullOrEmpty(_options.RunnerDataDir))
        {
            return _options.RunnerDataDir;
        }
        var envDir = Environment.GetEnvironmentVariable("CRONIQ_RUNNER_DATA_DIR");
        if (!string.IsNullOrEmpty(envDir))
        {
            return envDir;
        }
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        return Path.Combine(string.IsNullOrEmpty(local) ? "/tmp" : local, "croniq-runner");
    }

    private static string GenerateId(string prefix)
    {
        var slug = Guid.NewGuid().ToString("N", CultureInfo.InvariantCulture)[..8];
        return $"{prefix}-{slug}";
    }
}
