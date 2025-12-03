using System.Collections.Generic;
using System.Diagnostics;
using Microsoft.Extensions.Logging;

namespace Croniq.Sdk;

public interface IJobExecutionContext
{
    string JobKey { get; }

    IReadOnlyDictionary<string, string> Metadata { get; }

    ILogger Logger { get; }

    ActivitySource ActivitySource { get; }
}
