using System;

namespace Croniq.Core.Scheduling;

internal static class SystemTime
{
    public static DateTimeOffset UtcNow() => DateTimeOffset.UtcNow;
    public static DateTimeOffset Now() => DateTimeOffset.Now;
}
