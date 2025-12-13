using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

/// <summary>
/// Optional hook to forward execution logs to external sinks (e.g., OTLP).
/// </summary>
public interface IExecutionLogExporter
{
    Task ExportAsync(IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken);
}
