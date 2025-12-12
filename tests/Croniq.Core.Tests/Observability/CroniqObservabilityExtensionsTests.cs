using System.Collections.Generic;
using Croniq.Core;
using Croniq.Core.Options;
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

    private sealed class TestLoggingBuilder : ILoggingBuilder
    {
        public TestLoggingBuilder(IServiceCollection services)
        {
            Services = services;
        }

        public IServiceCollection Services { get; }
    }
}
