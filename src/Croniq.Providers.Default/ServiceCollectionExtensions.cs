using System;
using Croniq.Providers.Default.Logging;
using Croniq.Providers.Default.Secrets;
using Croniq.Providers.Default.Telemetry;
using Croniq.Providers.Logging;
using Croniq.Providers.Secrets;
using Croniq.Providers.Telemetry;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq.Providers.Default;

/// <summary>
/// DI helpers to wire default provider implementations.
/// </summary>
public static class ServiceCollectionExtensions
{
    /// <summary>
    /// Registers the default logging/telemetry providers; secrets must be added separately via <see cref="AddCroniqInMemorySecrets"/>.
    /// </summary>
    public static IServiceCollection AddCroniqDefaultProviders(this IServiceCollection services)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));

        services.TryAddSingleton<ILoggingProvider, LoggerFactoryProvider>();
        services.TryAddSingleton<ITelemetryProvider, DefaultTelemetryProvider>();

        return services;
    }

    /// <summary>
    /// Registers the in-memory secret provider for development/testing scenarios.
    /// </summary>
    public static IServiceCollection AddCroniqInMemorySecrets(this IServiceCollection services, Action<InMemorySecretProviderOptions>? configure = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));

        services.AddOptions<InMemorySecretProviderOptions>();
        if (configure is not null)
        {
            services.Configure(configure);
        }

        services.TryAddSingleton<ISecretProvider, InMemorySecretProvider>();
        return services;
    }
}
