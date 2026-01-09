using System.IO;
using System.Reflection;
using Croniq.Core;
using Croniq.Core.Jobs;
using Croniq.Hosting;
using Croniq.Sdk;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Hosting.Tests;

public sealed class JobAssemblyConfigurationExtensionsTests
{
    [Fact]
    public void AddCroniqJobsFromConfiguration_registers_jobs_from_array()
    {
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Jobs:Assemblies:0"] = Assembly.GetExecutingAssembly().Location
            })
            .Build();

        var services = new ServiceCollection();
        services.AddCroniqCore();

        services.AddCroniqJobsFromConfiguration(config);

        var provider = services.BuildServiceProvider();
        var registry = provider.GetRequiredService<IJobRegistry>();
        registry.TryGet(JobKey.Create("hosting", "sample"), out _).ShouldBeTrue();
    }

    [Fact]
    public void AddCroniqJobsFromConfiguration_splits_delimited_list_and_skips_duplicates()
    {
        var location = Assembly.GetExecutingAssembly().Location;
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Jobs:Assemblies"] = $"{location};{location}"
            })
            .Build();

        var services = new ServiceCollection();
        services.AddCroniqCore();

        services.AddCroniqJobsFromConfiguration(config);

        var provider = services.BuildServiceProvider();
        provider.GetRequiredService<IJobRegistry>().Descriptors.Count.ShouldBe(1);
    }

    [Fact]
    public void AddCroniqJobsFromConfiguration_throws_for_missing_assembly()
    {
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Jobs:Assemblies:0"] = "missing.dll"
            })
            .Build();

        var services = new ServiceCollection();
        services.AddCroniqCore();

        Should.Throw<FileNotFoundException>(() => services.AddCroniqJobsFromConfiguration(config));
    }

    [CroniqJob("hosting", "sample")]
    private sealed class SampleJob : IJob
    {
        public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken = default) => Task.CompletedTask;
    }
}
