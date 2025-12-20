using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Options;
using Croniq.Sdk;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Jobs;

public class JobHandlerRegistryTests
{
    [Fact]
    public async Task TryGet_ReturnsRegisteredHandler()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "tenant", EnvironmentTag = "dev" });
        var attribute = new CroniqJobAttribute("samples", "demo");
        var handled = false;
        Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task> handler = (_, _, _) =>
        {
            handled = true;
            return Task.CompletedTask;
        };

        var registry = new JobHandlerRegistry(options, new[]
        {
            new JobHandlerRegistration(attribute, handler)
        });

        var jobKey = JobKey.Create("tenant", "dev", "samples", "demo").Value;
        registry.TryGet(jobKey, out var resolved).ShouldBeTrue();

        var context = new TestExecutionContext(jobKey);
        await resolved(new ServiceCollection().BuildServiceProvider(), context, CancellationToken.None);

        handled.ShouldBeTrue();
    }

    [Fact]
    public void TryGet_ReturnsFalse_ForBlankKey()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions());
        var registry = new JobHandlerRegistry(options, Array.Empty<JobHandlerRegistration>());

        registry.TryGet(" ", out _).ShouldBeFalse();
    }

    [Fact]
    public void Throws_OnDuplicateJobKeys()
    {
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = "tenant", EnvironmentTag = "dev" });
        var attribute = new CroniqJobAttribute("samples", "demo");
        var handler = new Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task>((_, _, _) => Task.CompletedTask);

        Should.Throw<InvalidOperationException>(() =>
            new JobHandlerRegistry(options, new[]
            {
                new JobHandlerRegistration(attribute, handler),
                new JobHandlerRegistration(attribute, handler)
            }));
    }

    [Fact]
    public void JobHandlerRegistration_RequiresArguments()
    {
        var attribute = new CroniqJobAttribute("samples", "demo");
        var handler = new Func<IServiceProvider, IJobExecutionContext, CancellationToken, Task>((_, _, _) => Task.CompletedTask);

        Should.Throw<ArgumentNullException>(() => new JobHandlerRegistration(null!, handler));
        Should.Throw<ArgumentNullException>(() => new JobHandlerRegistration(attribute, null!));
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
