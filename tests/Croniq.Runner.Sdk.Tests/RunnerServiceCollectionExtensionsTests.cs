using System;
using System.Linq;
using Croniq.Runner;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Runner.Sdk.Tests;

public sealed class RunnerServiceCollectionExtensionsTests
{
    [Fact]
    public void AddCroniqRunner_Throws_WhenConfigMissing()
    {
        var services = new ServiceCollection();

        Should.Throw<InvalidOperationException>(() =>
        {
            services.AddCroniqRunner(_ => { });
        });
    }

    [Fact]
    public void AddCroniqRunner_Throws_WhenNoHandlers()
    {
        var services = new ServiceCollection();

        Should.Throw<InvalidOperationException>(() =>
        {
            services.AddCroniqRunner(options =>
            {
                options.Config = CreateConfig();
            });
        });
    }

    [Fact]
    public void AddCroniqRunner_RegistersRunnerAndHostedService()
    {
        var services = new ServiceCollection();
        services.AddLogging();

        services.AddCroniqRunnerHostedService(options =>
        {
            options.Config = CreateConfig();
            options.OnExecute("demo-job", (_, _, _, _) => Task.CompletedTask);
        });

        var provider = services.BuildServiceProvider();
        provider.GetRequiredService<CroniqRunner>().ShouldNotBeNull();

        var hosted = provider.GetServices<IHostedService>()
            .OfType<CroniqRunnerHostedService>()
            .ToArray();
        hosted.Length.ShouldBe(1);
    }

    private static RunnerConfig CreateConfig()
        => new()
        {
            BaseUrl = "http://localhost:5080",
            TenantId = "tenant",
            Environment = "dev",
            RunnerId = "runner",
            ApiKey = "ak_test"
        };
}
