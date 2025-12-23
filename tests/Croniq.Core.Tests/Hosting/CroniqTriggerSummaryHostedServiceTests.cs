using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Hosting;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using NSubstitute;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public sealed class CroniqTriggerSummaryHostedServiceTests
{
    [Fact]
    public async Task Skips_summary_in_validate_mode()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantReference = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });
        var startupOptions = Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Validate" });

        var service = new CroniqTriggerSummaryHostedService(
            store,
            options,
            startupOptions,
            NullLogger<CroniqTriggerSummaryHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.DidNotReceive().ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>());
    }

    [Fact]
    public async Task Lists_triggers_and_computes_summary_in_run_mode()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var scope = new PartitionScope("t", "dev");
        var triggers = new List<TriggerDefinition>
        {
            new("trigger-1", "t:dev:samples:job", "0 * * * * ?", scope),
            new("trigger-2", "t:dev:samples:job", "0/5 * * * * ?", scope, Enabled: false)
        };
        store.ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(triggers));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantReference = "t",
            EnvironmentTag = "dev",
            InstanceId = "i1"
        });
        var startupOptions = Microsoft.Extensions.Options.Options.Create(new CroniqStartupOptions { Mode = "Run" });

        var service = new CroniqTriggerSummaryHostedService(
            store,
            options,
            startupOptions,
            NullLogger<CroniqTriggerSummaryHostedService>.Instance);

        await service.StartAsync(CancellationToken.None);

        await store.Received(1).ListTriggersAsync(
            Arg.Is<PartitionScope>(s => s.TenantId == "t" && s.EnvironmentTag == "dev"),
            Arg.Any<CancellationToken>());
    }
}
