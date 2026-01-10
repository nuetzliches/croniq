using System.Collections.Generic;
using System.Linq;
using Croniq.Core;
using Croniq.Options;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Observability;

public class CroniqObservabilityExtensionsTests
{
    [Fact]
    public void Configures_tracing_metrics_and_logging()
    {
        var settings = new Dictionary<string, string?>
        {
            ["Croniq:Core:EnvironmentTag"] = "prod",
            ["Croniq:Core:TenantId"] = "acme",
            ["Croniq:Observability:OtlpEndpoint"] = "http://localhost:4317",
            ["Croniq:Observability:OtlpProtocol"] = "grpc",
            ["Croniq:Observability:EnableHttp2UnencryptedSupport"] = "true"
        };

        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(settings)
            .Build();

        var services = new ServiceCollection();
        ILoggingBuilder logging = new TestLoggingBuilder(services);

        var builder = services.AddCroniqObservability(configuration, logging, "croniq-api", opts =>
        {
            opts.EnableConsoleLogging = true;
            opts.EnableOtlpLogExport = true;
            opts.ResourceAttributes["region"] = "eu";
        });

        builder.ShouldNotBeNull();
        services.BuildServiceProvider().ShouldNotBeNull();
    }

    [Fact]
    public void Applies_default_minimum_level_overrides_to_logging_filters()
    {
        var settings = new Dictionary<string, string?>
        {
            ["Croniq:Observability:EnableLogging"] = "false"
        };

        var configuration = new ConfigurationBuilder()
            .AddInMemoryCollection(settings)
            .Build();

        var services = new ServiceCollection();
        services.AddOptions();
        ILoggingBuilder logging = new TestLoggingBuilder(services);

        services.AddCroniqObservability(configuration, logging, "croniq-api");

        var provider = services.BuildServiceProvider();
        var filterOptions = provider.GetRequiredService<IOptions<LoggerFilterOptions>>().Value;

        var efCoreRule = filterOptions.Rules.FirstOrDefault(rule =>
            string.Equals(rule.CategoryName, "Microsoft.EntityFrameworkCore.Database.Command", StringComparison.Ordinal));

        efCoreRule.ShouldNotBeNull();
        efCoreRule!.LogLevel.ShouldBe(LogLevel.Warning);
    }

    private sealed class TestLoggingBuilder : ILoggingBuilder
    {
        public TestLoggingBuilder(IServiceCollection services)
        {
            Services = services;
        }

        public IServiceCollection Services { get; }
    }
}
