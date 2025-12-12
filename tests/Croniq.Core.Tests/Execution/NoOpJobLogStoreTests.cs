using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class NoOpJobLogStoreTests
{
    [Fact]
    public async Task Completes_without_throwing()
    {
        var store = new NoOpJobLogStore();
        var record = new JobExecutionRecord(
            "exec-1",
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

        var entry = new JobLogEntry(
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

        var completion = new JobExecutionCompletion(
            "exec-1",
            DateTimeOffset.UtcNow,
            JobExecutionStatus.Succeeded,
            12.3,
            null,
            null);

        await store.OnExecutionStartedAsync(record, CancellationToken.None);
        await store.AppendAsync("exec-1", new[] { entry }, CancellationToken.None);
        await store.OnExecutionCompletedAsync(completion, CancellationToken.None);

        // If we reached here without exceptions, the no-op store behaved as expected.
        true.ShouldBeTrue();
    }
}
