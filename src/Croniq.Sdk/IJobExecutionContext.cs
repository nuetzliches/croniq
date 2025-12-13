using System.Collections.Generic;
using System.Diagnostics;
using Microsoft.Extensions.Logging;

namespace Croniq.Sdk;

public interface IJobExecutionContext
{
    /// <summary>
    /// Unique identifier for the current job execution.
    /// </summary>
    string ExecutionId { get; }

    string JobKey { get; }

    IReadOnlyDictionary<string, string> Metadata { get; }

    ILogger Logger { get; }

    ActivitySource ActivitySource { get; }
}
