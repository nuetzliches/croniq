using System;
using Microsoft.Extensions.Logging;

namespace Croniq.Persistence.Postgres.Tests;

internal static class TestLogging
{
    private const string LogLevelEnvVar = "CRONIQ_TEST_LOGLEVEL";
    private const string EfDiagnosticsEnvVar = "CRONIQ_TEST_EF_VERBOSE";

    public static void Configure(ILoggingBuilder builder)
    {
        var logLevel = ResolveLogLevel();
        builder.SetMinimumLevel(logLevel);
        builder.AddFilter("Microsoft.EntityFrameworkCore", logLevel);
        builder.AddSimpleConsole(options =>
        {
            options.SingleLine = true;
            options.TimestampFormat = "HH:mm:ss ";
            options.IncludeScopes = false;
        });
    }

    public static bool EnableVerboseEfDiagnostics()
    {
        var value = Environment.GetEnvironmentVariable(EfDiagnosticsEnvVar);
        return string.Equals(value, "true", StringComparison.OrdinalIgnoreCase);
    }

    private static LogLevel ResolveLogLevel()
    {
        var value = Environment.GetEnvironmentVariable(LogLevelEnvVar);
        return Enum.TryParse(value, ignoreCase: true, out LogLevel parsed)
            ? parsed
            : LogLevel.Warning;
    }
}


