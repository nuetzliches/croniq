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
    private const string TestTenantId = "00000000-0000-0000-0000-000000000001";

    [Fact]
    public async Task Skips_summary_in_validate_mode()
    {
        var store = Substitute.For<IJobPersistenceProvider>();
        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantMode = TenantMode.Multi,
            TenantId = TestTenantId,
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
        var scope = new PartitionScope(TestTenantId, "dev");
        var triggers = new List<TriggerDefinition>
        {
            new("trigger-1", $"{TestTenantId}:dev:samples:job", "0 * * * * ?", scope),
            new("trigger-2", $"{TestTenantId}:dev:samples:job", "0/5 * * * * ?", scope, Enabled: false)
        };
        store.ListTriggersAsync(Arg.Any<PartitionScope>(), Arg.Any<CancellationToken>())
            .Returns(Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(triggers));

        var options = Microsoft.Extensions.Options.Options.Create(new CroniqOptions
        {
            TenantMode = TenantMode.Multi,
            TenantId = TestTenantId,
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
            Arg.Is<PartitionScope>(s => s.TenantId == TestTenantId && s.EnvironmentTag == "dev"),
            Arg.Any<CancellationToken>());
    }
}
