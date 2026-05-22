using System.Diagnostics.CodeAnalysis;
using System.Text.Json;

using Croniq.Runner.Sdk.Internal;

namespace Croniq.Runner.Sdk.ShellExec;

/// <summary>
/// Decodes the <c>__runner_exec</c> metadata key into a typed
/// <see cref="RunnerExec"/>. The server stores the payload as a JSON-encoded
/// <em>string</em> inside the metadata map — this helper unwraps it.
/// </summary>
public static class CroniqShellDecoder
{
    /// <summary>The metadata key set by the Croniqfile compiler.</summary>
    public const string MetadataKey = "__runner_exec";

    /// <summary>
    /// Try to decode <c>metadata["__runner_exec"]</c>. Returns <c>false</c>
    /// without throwing if the key is missing or malformed.
    /// </summary>
    public static bool TryDecode(
        JsonElement metadata,
        [NotNullWhen(true)] out RunnerExec? exec,
        out string? error)
    {
        exec = null;
        if (metadata.ValueKind != JsonValueKind.Object || !metadata.TryGetProperty(MetadataKey, out var raw))
        {
            error = "metadata does not contain __runner_exec";
            return false;
        }
        if (raw.ValueKind != JsonValueKind.String)
        {
            error = "__runner_exec is not a string";
            return false;
        }

        var json = raw.GetString();
        if (string.IsNullOrEmpty(json))
        {
            error = "__runner_exec is empty";
            return false;
        }

        try
        {
            exec = JsonSerializer.Deserialize(json, CroniqJsonContext.Default.RunnerExec);
            if (exec is null)
            {
                error = "__runner_exec decoded to null";
                return false;
            }
            error = null;
            return true;
        }
        catch (JsonException ex)
        {
            error = ex.Message;
            return false;
        }
    }
}
