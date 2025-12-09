using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Croniq.TestKit.Infrastructure;
using DotNet.Testcontainers.Containers;

namespace Croniq.TestKit.Diagnostics;

/// <summary>
/// Persists Docker logs from Testcontainers instances so CI can surface them as artifacts.
/// </summary>
public static class TestcontainerLogCollector
{
    public static async Task<string> CaptureContainerLogsAsync(
        ITestcontainersContainer container,
        string artifactName,
        CancellationToken cancellationToken = default)
    {
        if (container is null) throw new ArgumentNullException(nameof(container));
        if (string.IsNullOrWhiteSpace(artifactName)) throw new ArgumentException("Artifact name is required.", nameof(artifactName));

        var sanitized = Sanitize(artifactName);
        var directory = RepositoryLocator.GetArtifactsDirectory(Path.Combine("containers", sanitized));
        var filePath = Path.Combine(directory, $"{sanitized}.log");

        var logs = await TryGetLogsAsync(container, cancellationToken).ConfigureAwait(false)
            ?? $"[{DateTime.UtcNow:O}] Container log capture is not available for the current Testcontainers version.";

        await File.WriteAllTextAsync(filePath, logs, cancellationToken).ConfigureAwait(false);

        return filePath;
    }

    private static async Task<string?> TryGetLogsAsync(ITestcontainersContainer container, CancellationToken cancellationToken)
    {
        var method = container.GetType()
            .GetMethods(BindingFlags.Instance | BindingFlags.Public)
            .FirstOrDefault(m => string.Equals(m.Name, "GetLogsAsync", StringComparison.Ordinal));

        if (method is null)
        {
            return null;
        }

        var parameters = method.GetParameters();
        var arguments = new object?[parameters.Length];
        for (var i = 0; i < parameters.Length; i++)
        {
            var parameter = parameters[i];
            if (parameter.ParameterType == typeof(CancellationToken))
            {
                arguments[i] = cancellationToken;
            }
            else if (parameter.HasDefaultValue)
            {
                arguments[i] = parameter.DefaultValue;
            }
            else if (parameter.ParameterType == typeof(bool))
            {
                arguments[i] = true;
            }
            else
            {
                arguments[i] = parameter.ParameterType.IsValueType
                    ? Activator.CreateInstance(parameter.ParameterType)
                    : null;
            }
        }

        var invocation = method.Invoke(container, arguments);
        switch (invocation)
        {
            case Task<string> stringTask:
                return await stringTask.ConfigureAwait(false);
            case Task task:
                await task.ConfigureAwait(false);
                return null;
            case string text:
                return text;
            default:
                return invocation?.ToString();
        }
    }

    private static string Sanitize(string value)
    {
        var invalid = Path.GetInvalidFileNameChars();
        var builder = new StringBuilder(value.Length);
        foreach (var c in value)
        {
            builder.Append(invalid.Contains(c) ? '-' : char.ToLowerInvariant(c));
        }

        var sanitized = builder.ToString().Trim('-');
        return string.IsNullOrEmpty(sanitized) ? "container" : sanitized;
    }
}
