using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Polly;

namespace Croniq.Core.Execution;

public interface IExecutionPolicyPipelineProvider
{
    ResiliencePipeline Get(JobKey jobKey, ExecutionPolicyOptions options);
}
