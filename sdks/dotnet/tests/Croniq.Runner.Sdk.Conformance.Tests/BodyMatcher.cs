using System.Text.Json;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Subset matcher with one wildcard token (<c>"*"</c>). Used to assert
/// request bodies and headers without forcing tests to mirror every
/// SDK-emitted field.
/// </summary>
internal static class BodyMatcher
{
    /// <summary>
    /// Returns null on success, otherwise a human-readable path-rooted
    /// diff explaining the first mismatch found.
    /// </summary>
    public static string? Match(object? expected, JsonElement actual, string path = "$")
    {
        if (expected is null)
        {
            if (actual.ValueKind == JsonValueKind.Null) return null;
            return $"{path}: expected null but got {actual.ValueKind}";
        }

        if (expected is string s && s == "*")
        {
            // Any non-empty value of any kind.
            return actual.ValueKind switch
            {
                JsonValueKind.Null => $"{path}: expected non-empty wildcard match but got null",
                JsonValueKind.String when string.IsNullOrEmpty(actual.GetString()) => $"{path}: expected non-empty string but got empty",
                _ => null,
            };
        }

        return expected switch
        {
            IDictionary<string, object?> dict => MatchObject(dict, actual, path),
            IList<object?> list => MatchArray(list, actual, path),
            string str => MatchScalar(str, actual, path),
            bool b => MatchBool(b, actual, path),
            long l => MatchLong(l, actual, path),
            int i => MatchLong(i, actual, path),
            double d => MatchDouble(d, actual, path),
            _ => MatchScalar(expected.ToString() ?? "", actual, path),
        };
    }

    private static string? MatchObject(IDictionary<string, object?> expected, JsonElement actual, string path)
    {
        if (actual.ValueKind != JsonValueKind.Object)
        {
            return $"{path}: expected object but got {actual.ValueKind}";
        }
        foreach (var kv in expected)
        {
            if (!actual.TryGetProperty(kv.Key, out var child))
            {
                return $"{path}.{kv.Key}: missing key";
            }
            var err = Match(kv.Value, child, $"{path}.{kv.Key}");
            if (err is not null) return err;
        }
        return null;
    }

    private static string? MatchArray(IList<object?> expected, JsonElement actual, string path)
    {
        if (actual.ValueKind != JsonValueKind.Array)
        {
            return $"{path}: expected array but got {actual.ValueKind}";
        }
        if (actual.GetArrayLength() != expected.Count)
        {
            return $"{path}: expected {expected.Count} item(s) but got {actual.GetArrayLength()}";
        }
        for (var i = 0; i < expected.Count; i++)
        {
            var err = Match(expected[i], actual[i], $"{path}[{i}]");
            if (err is not null) return err;
        }
        return null;
    }

    private static string? MatchScalar(string expected, JsonElement actual, string path)
    {
        if (actual.ValueKind != JsonValueKind.String)
        {
            return $"{path}: expected string '{expected}' but got {actual.ValueKind}";
        }
        var s = actual.GetString();
        return s == expected ? null : $"{path}: expected '{expected}' but got '{s}'";
    }

    private static string? MatchBool(bool expected, JsonElement actual, string path)
    {
        if (actual.ValueKind != JsonValueKind.True && actual.ValueKind != JsonValueKind.False)
        {
            return $"{path}: expected bool but got {actual.ValueKind}";
        }
        var b = actual.GetBoolean();
        return b == expected ? null : $"{path}: expected {expected} but got {b}";
    }

    private static string? MatchLong(long expected, JsonElement actual, string path)
    {
        if (actual.ValueKind != JsonValueKind.Number)
        {
            return $"{path}: expected number but got {actual.ValueKind}";
        }
        if (!actual.TryGetInt64(out var n))
        {
            return $"{path}: expected integer but got fractional number";
        }
        return n == expected ? null : $"{path}: expected {expected} but got {n}";
    }

    private static string? MatchDouble(double expected, JsonElement actual, string path)
    {
        if (actual.ValueKind != JsonValueKind.Number)
        {
            return $"{path}: expected number but got {actual.ValueKind}";
        }
        var n = actual.GetDouble();
        return Math.Abs(n - expected) < 1e-9 ? null : $"{path}: expected {expected} but got {n}";
    }
}
