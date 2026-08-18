using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;

using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

public class OptionsBindingTests
{
    [Fact]
    public void AddCroniqRunner_BindsFromIConfiguration()
    {
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Runner:ServerUrl"] = "https://example.test:4000",
                ["Croniq:Runner:ApiKey"] = "croniq_abc",
                ["Croniq:Runner:MaxInflight"] = "12",
                ["Croniq:Runner:Capabilities:0"] = "billing",
                ["Croniq:Runner:Tags:0"] = "lang=dotnet",
            })
            .Build();

        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqRunner(config.GetSection(CroniqRunnerOptions.SectionName));

        var provider = services.BuildServiceProvider();
        var opts = provider.GetRequiredService<IOptions<CroniqRunnerOptions>>().Value;

        opts.ServerUrl.ShouldBe("https://example.test:4000");
        opts.ApiKey.ShouldBe("croniq_abc");
        opts.MaxInflight.ShouldBe(12);
        opts.Capabilities.ShouldContain("billing");
        opts.Tags.ShouldContain("lang=dotnet");
    }
}
