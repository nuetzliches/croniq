using System;
using System.Globalization;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.TestKit.Containers;

/// <summary>
/// Utility methods for persisting Testcontainers log streams to disk when diagnosing failures.
/// </summary>
public static class TestcontainerLogCollector
{
    public static async Task<string?> TryWriteLogsAsync(Stream? logStream, string outputDirectory, string filePrefix, CancellationToken cancellationToken = default)
    {
        if (logStream is null)
        {
            return null;
        }

        Directory.CreateDirectory(outputDirectory);
        var timestamp = DateTimeOffset.UtcNow.ToString("yyyyMMddHHmmss", CultureInfo.InvariantCulture);
        var filePath = Path.Combine(outputDirectory, $"{filePrefix}-{timestamp}.log");

        logStream.Position = 0;
        await using var fileStream = File.Create(filePath);
        await logStream.CopyToAsync(fileStream, cancellationToken).ConfigureAwait(false);

        return filePath;
    }
}
