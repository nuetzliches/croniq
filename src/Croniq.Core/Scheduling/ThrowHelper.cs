using System;

namespace Croniq.Core.Scheduling;

internal static class Throw
{
    public static void FormatException(string message, Exception inner) =>
        throw new FormatException(message, inner);

    public static void ArgumentException(string message, string paramName) =>
        throw new ArgumentException(message, paramName);

    public static T ArgumentException<T>(string message, string? paramName = null) =>
        throw new ArgumentException(message, paramName);

    public static T ArgumentOutOfRangeException<T>(string message, string? paramName = null) =>
        throw new ArgumentOutOfRangeException(paramName, message);

    public static void NotSupportedException(string message) =>
        throw new NotSupportedException(message);

    public static void FormatException(string message) =>
        throw new FormatException(message);
}
