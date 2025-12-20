using Croniq.Api;
using Croniq.Webhooks;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class ObservabilityExtensionsTests
{
    [Fact]
    public void AddCroniqApiObservability_ConfiguresTracingAndMetrics()
    {
        var services = new ServiceCollection();
        var configuration = new ConfigurationBuilder().Build();
        ILoggingBuilder logging = new TestLoggingBuilder(services);
        var tracingConfigured = false;
        var metricsConfigured = false;

        var builder = services.AddCroniqApiObservability(
            configuration,
            logging,
            configureTracing: _ => tracingConfigured = true,
            configureMetrics: _ => metricsConfigured = true);

        builder.ShouldNotBeNull();
        tracingConfigured.ShouldBeTrue();
        metricsConfigured.ShouldBeTrue();
    }

    [Fact]
    public void AddCroniqWebhookObservability_ConfiguresTracingAndMetrics()
    {
        var services = new ServiceCollection();
        var configuration = new ConfigurationBuilder().Build();
        ILoggingBuilder logging = new TestLoggingBuilder(services);
        var tracingConfigured = false;
        var metricsConfigured = false;

        var builder = services.AddCroniqWebhookObservability(
            configuration,
            logging,
            configureTracing: _ => tracingConfigured = true,
            configureMetrics: _ => metricsConfigured = true);

        builder.ShouldNotBeNull();
        tracingConfigured.ShouldBeTrue();
        metricsConfigured.ShouldBeTrue();
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
