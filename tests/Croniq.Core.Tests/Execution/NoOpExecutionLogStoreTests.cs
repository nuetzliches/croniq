using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class NoOpExecutionLogStoreTests
{
    [Fact]
    public async Task Completes_without_throwing()
    {
        var store = new NoOpExecutionLogStore();
        var record = new ExecutionRecord(
            "exec-1",
            ExecutionKind.Job,
            null,
            "t:env:ns:job",
            "t",
            "env",
            "tr-1",
            DateTimeOffset.UtcNow,
            DateTimeOffset.UtcNow,
            "instance-1",
            "trace",
            "span",
            "corr");

        var entry = new ExecutionLogEntry(
            "exec-1",
            DateTimeOffset.UtcNow,
            LogLevel.Information,
            "Hello",
            "Hello",
            null,
            new Dictionary<string, object?>(),
            "trace",
            "span",
            "corr",
            1);

        var completion = new ExecutionCompletion(
            "exec-1",
            DateTimeOffset.UtcNow,
            ExecutionStatus.Succeeded,
            12.3,
            null,
            null);

        await store.OnExecutionStartedAsync(record, CancellationToken.None);
        await store.AppendAsync("exec-1", new[] { entry }, CancellationToken.None);
        await store.OnExecutionCompletedAsync(completion, CancellationToken.None);

        true.ShouldBeTrue();
    }
}
