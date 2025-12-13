using System.Collections.Generic;
using System.Threading;

namespace Croniq.Core.Execution;

public sealed class NoOpExecutionLogReader : IExecutionLogReader
{
    public async IAsyncEnumerable<string> ReadLinesAsync(string executionId, [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken cancellationToken)
    {
        await Task.CompletedTask.ConfigureAwait(false);
        yield break;
    }
}
