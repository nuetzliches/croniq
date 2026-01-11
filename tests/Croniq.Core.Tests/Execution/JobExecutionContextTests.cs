using System.Collections.Generic;
using System.Diagnostics;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class JobExecutionContextTests
{
    [Fact]
    public void Uses_defaults_when_metadata_or_logger_missing()
    {
        var ctx = new JobExecutionContext("exec-1", "ns:job", null, null!, null!);
        ctx.ExecutionId.ShouldBe("exec-1");
        ctx.JobKey.ShouldBe("ns:job");
        ctx.Metadata.ShouldBeEmpty();
        ctx.Logger.ShouldBe(NullLogger.Instance);
        ctx.ActivitySource.ShouldNotBeNull();

        var metadata = new Dictionary<string, string> { { "foo", "bar" } };
        var ctxWithValues = new JobExecutionContext("exec-2", "a:b:c", metadata, NullLogger.Instance, new ActivitySource("test"));
        ctxWithValues.Metadata.ShouldContainKey("foo");
        ctxWithValues.ExecutionId.ShouldBe("exec-2");
    }
}
