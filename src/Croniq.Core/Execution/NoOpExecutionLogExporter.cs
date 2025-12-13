using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

public sealed class NoOpExecutionLogExporter : IExecutionLogExporter
{
    public Task ExportAsync(IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken) => Task.CompletedTask;
}
