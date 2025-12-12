using System;
using System.Diagnostics;
using Croniq.Providers.Default;
using Croniq.Providers.Default.Secrets;
using Croniq.Providers.Logging;
using Croniq.Providers.Secrets;
using Croniq.Providers.Telemetry;
using Shouldly;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Xunit;

namespace Croniq.Providers.Default.Tests;

public class ProviderTests
{
    [Fact]
    public void Logging_provider_creates_logger()
    {
        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqDefaultProviders();
        var sp = services.BuildServiceProvider();

        var provider = sp.GetRequiredService<ILoggingProvider>();
        var logger = provider.CreateLogger("test-category");

        logger.ShouldNotBeNull();
    }

    [Fact]
    public void Telemetry_provider_caches_instances()
    {
        var telemetry = new Default.Telemetry.DefaultTelemetryProvider();
        var s1 = telemetry.GetActivitySource("Croniq", "1.0");
        var s2 = telemetry.GetActivitySource("Croniq", "1.0");
        var m1 = telemetry.GetMeter("Croniq", "1.0");
        var m2 = telemetry.GetMeter("Croniq", "1.0");

        s1.ShouldBeSameAs(s2);
        m1.ShouldBeSameAs(m2);
    }

    [Fact]
    public async Task Secret_provider_returns_seed_and_env()
    {
        var services = new ServiceCollection();
        services.AddCroniqInMemorySecrets(opts => opts.Secrets["api-key"] = "123");
        var sp = services.BuildServiceProvider();

        var provider = sp.GetRequiredService<ISecretProvider>();

        var seeded = await provider.GetSecretAsync(new SecretRequest("api-key"));
        seeded.ShouldNotBeNull();
        seeded!.Value.ShouldBe("123");

        try
        {
            Environment.SetEnvironmentVariable("TEST_SECRET", "env-value");
            var fromEnv = await provider.GetSecretAsync(new SecretRequest("secret", Scope: "test"));
            fromEnv.ShouldNotBeNull();
            fromEnv!.Value.ShouldBe("env-value");
        }
        finally
        {
            Environment.SetEnvironmentVariable("TEST_SECRET", null);
        }
    }
}
