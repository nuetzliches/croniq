using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Sdk;

public interface IJob
{
    Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken);
}
