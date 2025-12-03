using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

public interface IJobExecutionPipeline
{
    Task ExecuteAsync(JobExecutionRequest request, CancellationToken cancellationToken);
}
