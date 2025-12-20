using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Jobs;

public class DelegatingJobTests
{
    [Fact]
    public async Task ExecuteAsync_InvokesRegisteredHandler()
    {
        var jobKey = "tenant:dev:samples:demo";
        var registry = new TestRegistry(jobKey, (_, _, _) => Task.CompletedTask);
        var job = new DelegatingJob(registry, new ServiceCollection().BuildServiceProvider());

        var context = new TestExecutionContext(jobKey);
        await job.ExecuteAsync(context, CancellationToken.None);

        registry.Calls.ShouldBe(1);
    }

    [Fact]
    public void ExecuteAsync_Throws_WhenContextIsNull()
    {
        var registry = new TestRegistry("job", (_, _, _) => Task.CompletedTask);
        var job = new DelegatingJob(registry, new ServiceCollection().BuildServiceProvider());

        Should.Throw<ArgumentNullException>(() => job.ExecuteAsync(null!, CancellationToken.None));
    }

    [Fact]
    public void ExecuteAsync_Throws_WhenHandlerMissing()
    {
        var registry = new TestRegistry("known", (_, _, _) => Task.CompletedTask);
        var job = new DelegatingJob(registry, new ServiceCollection().BuildServiceProvider());

        Should.Throw<InvalidOperationException>(() => job.ExecuteAsync(new TestExecutionContext("missing"), CancellationToken.None));
    }

    private sealed class TestRegistry : IJobHandlerRegistry
    {
        private readonly string _jobKey;
        private readonly Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> _handler;

        public TestRegistry(string jobKey, Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler)
        {
            _jobKey = jobKey;
            _handler = handler;
        }

        public int Calls { get; private set; }

        public bool TryGet(string jobKey, out Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler)
        {
            if (string.Equals(jobKey, _jobKey, StringComparison.Ordinal))
            {
                handler = (sp, ctx, ct) =>
                {
                    Calls++;
                    return _handler(sp, ctx, ct);
                };
                return true;
            }

            handler = null!;
            return false;
        }
    }

    private sealed class TestExecutionContext : IJobExecutionContext
    {
        public TestExecutionContext(string jobKey)
        {
            JobKey = jobKey;
        }

        public string ExecutionId { get; } = "exec-1";
        public string JobKey { get; }
        public IReadOnlyDictionary<string, string> Metadata { get; } = new Dictionary<string, string>();
        public Microsoft.Extensions.Logging.ILogger Logger { get; } = NullLogger.Instance;
        public ActivitySource ActivitySource { get; } = new("Croniq.Core.Tests");
    }
}
