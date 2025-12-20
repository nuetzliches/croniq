using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class NoOpExecutionComponentsTests
{
    [Fact]
    public async Task NoOpExecutionHistoryReader_ReturnsEmptyResults()
    {
        var reader = new NoOpExecutionHistoryReader();
        var scope = new PartitionScope("tenant", "dev");

        var executions = await reader.ListExecutionsAsync(scope, null, CancellationToken.None);
        var execution = await reader.GetExecutionAsync("exec-1", CancellationToken.None);

        executions.ShouldBeEmpty();
        execution.ShouldBeNull();
    }

    [Fact]
    public async Task NoOpExecutionLogExporter_Completes()
    {
        var exporter = new NoOpExecutionLogExporter();

        await exporter.ExportAsync(new List<ExecutionLogEntry>(), CancellationToken.None);
    }

    [Fact]
    public async Task NoOpExecutionLogReader_ProducesNoLines()
    {
        var reader = new NoOpExecutionLogReader();
        var lines = new List<string>();

        await foreach (var line in reader.ReadLinesAsync("exec-1", CancellationToken.None))
        {
            lines.Add(line);
        }

        lines.ShouldBeEmpty();
    }
}
