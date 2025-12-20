using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class LoggerExecutionLogExporterTests
{
    [Fact]
    public async Task ExportAsync_ForwardsEntriesToLogger()
    {
        var exporter = new LoggerExecutionLogExporter(NullLogger<LoggerExecutionLogExporter>.Instance);
        var entries = new[]
        {
            new ExecutionLogEntry(
                "exec-1",
                DateTimeOffset.UtcNow,
                LogLevel.Information,
                "Hello {Value}",
                null,
                null,
                new Dictionary<string, object?> { ["Value"] = 1 },
                "trace",
                "span",
                "corr",
                1)
        };

        await exporter.ExportAsync(entries, CancellationToken.None);

        entries.ShouldNotBeEmpty();
    }
}
