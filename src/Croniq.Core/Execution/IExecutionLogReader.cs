using System.Collections.Generic;
using System.Threading;

namespace Croniq.Core.Execution;

/// <summary>
/// Abstraction for reading persisted execution logs.
/// </summary>
public interface IExecutionLogReader
{
    IAsyncEnumerable<string> ReadLinesAsync(string executionId, CancellationToken cancellationToken);
}
