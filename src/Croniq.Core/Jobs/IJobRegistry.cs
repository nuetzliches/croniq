using System.Collections.Generic;

namespace Croniq.Core.Jobs;

public interface IJobRegistry
{
    IReadOnlyCollection<JobDescriptor> Descriptors { get; }

    bool TryGet(JobKey jobKey, out JobDescriptor descriptor);
}
