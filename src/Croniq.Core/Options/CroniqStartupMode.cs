using System;

namespace Croniq.Options;

public enum CroniqStartupMode
{
    Run,
    Validate
}

internal static class CroniqStartupModeParser
{
    public static CroniqStartupMode Parse(string? mode)
    {
        if (string.IsNullOrWhiteSpace(mode))
        {
            return CroniqStartupMode.Run;
        }

        if (Enum.TryParse<CroniqStartupMode>(mode, ignoreCase: true, out var parsed))
        {
            return parsed;
        }

        throw new InvalidOperationException($"Croniq startup mode '{mode}' is invalid. Valid values: Run, Validate.");
    }

    public static bool TryParse(string? mode, out CroniqStartupMode parsed)
    {
        if (string.IsNullOrWhiteSpace(mode))
        {
            parsed = CroniqStartupMode.Run;
            return true;
        }

        return Enum.TryParse(mode, ignoreCase: true, out parsed);
    }
}
