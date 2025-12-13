using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using NSubstitute;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class ExecutionLogSinkProviderTests
{
    [Fact]
    public async Task Skips_when_no_execution_scope()
    {
        var store = Substitute.For<IExecutionLogStore>();
        var exporter = Substitute.For<IExecutionLogExporter>();
        var provider = new ExecutionLogSinkProvider(store, exporter, Microsoft.Extensions.Options.Options.Create(new ExecutionLogSinkOptions { BatchSize = 1, FlushInterval = TimeSpan.FromMilliseconds(50) }));
        var logger = provider.CreateLogger("test");

        logger.LogInformation("hello");

        await Task.Delay(100);
        await store.DidNotReceiveWithAnyArgs().AppendAsync(default!, default!, default);
        provider.Dispose();
    }

    [Fact]
    public async Task Persists_when_execution_scope_present()
    {
        var store = Substitute.For<IExecutionLogStore>();
        var exporter = Substitute.For<IExecutionLogExporter>();
        var provider = new ExecutionLogSinkProvider(store, exporter, Microsoft.Extensions.Options.Options.Create(new ExecutionLogSinkOptions { BatchSize = 1, FlushInterval = TimeSpan.FromMilliseconds(50) }));
        provider.SetScopeProvider(new LoggerExternalScopeProvider());
        var logger = provider.CreateLogger("test");

        using (logger.BeginScope(new Dictionary<string, object?>
               {
                   { "croniq.execution_id", "exec-1" },
                   { "croniq.job.key", "t:dev:ns:job" },
                   { "croniq.correlation_id", "corr-1" }
               }))
        {
            logger.LogInformation("Hello {User}", "alice");
        }

        await Task.Delay(150);
        await store.Received().AppendAsync(
            "exec-1",
            Arg.Is<IReadOnlyCollection<ExecutionLogEntry>>(c => c.Count == 1 && c.First().CorrelationId == "corr-1"),
            Arg.Any<CancellationToken>());
        await exporter.Received(1).ExportAsync(Arg.Any<IReadOnlyCollection<ExecutionLogEntry>>(), Arg.Any<CancellationToken>());
        provider.Dispose();
    }

    [Fact]
    public async Task Respects_minimum_level()
    {
        var store = Substitute.For<IExecutionLogStore>();
        var exporter = Substitute.For<IExecutionLogExporter>();
        var provider = new ExecutionLogSinkProvider(store, exporter, Microsoft.Extensions.Options.Options.Create(new ExecutionLogSinkOptions { MinimumLevel = LogLevel.Warning, BatchSize = 1 }));
        provider.SetScopeProvider(new LoggerExternalScopeProvider());
        var logger = provider.CreateLogger("test");

        using (logger.BeginScope(new Dictionary<string, object?>
               {
                   { "croniq.execution_id", "exec-2" }
               }))
        {
            logger.LogInformation("info");
            logger.LogError("error");
        }

        await Task.Delay(150);
        await store.Received(1).AppendAsync(
            "exec-2",
            Arg.Is<IReadOnlyCollection<ExecutionLogEntry>>(c => c.Count == 1 && c.First().Level == LogLevel.Error),
            Arg.Any<CancellationToken>());
        await exporter.Received(1).ExportAsync(Arg.Any<IReadOnlyCollection<ExecutionLogEntry>>(), Arg.Any<CancellationToken>());
        provider.Dispose();
    }
}
