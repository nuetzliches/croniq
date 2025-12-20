using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Sdk;

public interface IJobTrigger
{
    Task TriggerOnceAsync(
        string jobKey,
        IReadOnlyDictionary<string, string>? metadata = null,
        TimeSpan? delay = null,
        CancellationToken cancellationToken = default);
}
