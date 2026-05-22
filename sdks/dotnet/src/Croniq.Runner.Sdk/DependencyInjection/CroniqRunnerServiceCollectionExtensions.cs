using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Hosting;
using Croniq.Runner.Sdk.Internal;

using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.DependencyInjection;

/// <summary>
/// <c>IServiceCollection</c> extensions to register the Croniq runner.
/// </summary>
public static class CroniqRunnerServiceCollectionExtensions
{
    /// <summary>
    /// Register the Croniq runner with explicit option configuration.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqRunner(
        this IServiceCollection services,
        Action<CroniqRunnerOptions>? configureOptions = null)
    {
        ArgumentNullException.ThrowIfNull(services);

        var optionsBuilder = services
            .AddOptions<CroniqRunnerOptions>()
            .ValidateDataAnnotations()
            .ValidateOnStart();

        if (configureOptions is not null)
        {
            optionsBuilder.Configure(configureOptions);
        }

        RegisterCore(services);
        return new CroniqRunnerBuilder(services);
    }

    /// <summary>
    /// Register the Croniq runner binding options from a configuration
    /// section. Use with
    /// <c>builder.Configuration.GetSection(CroniqRunnerOptions.SectionName)</c>.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqRunner(
        this IServiceCollection services,
        IConfiguration configurationSection)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configurationSection);

        services
            .AddOptions<CroniqRunnerOptions>()
            .Bind(configurationSection)
            .ValidateDataAnnotations()
            .ValidateOnStart();

        RegisterCore(services);
        return new CroniqRunnerBuilder(services);
    }

    private static void RegisterCore(IServiceCollection services)
    {
        services.TryAddSingleton<RunnerStateProbe>();
        services.TryAddSingleton<IRunnerStateProbe>(sp => sp.GetRequiredService<RunnerStateProbe>());
        services.TryAddSingleton<RunnerIdentityResolver>();
        services.TryAddSingleton<CroniqHandlerRegistry>();

        services.AddHttpClient<ICroniqClient, CroniqClient>((sp, http) =>
        {
            var opts = sp.GetRequiredService<IOptions<CroniqRunnerOptions>>().Value;
            http.BaseAddress = new Uri(opts.ServerUrl.TrimEnd('/'));
            http.Timeout = Timeout.InfiniteTimeSpan;
        })
        .AddHttpMessageHandler<CroniqAuthHandler>();

        services.TryAddTransient<CroniqAuthHandler>();
        services.TryAddSingleton(sp => new CroniqRunner(
            sp.GetRequiredService<IOptions<CroniqRunnerOptions>>(),
            sp.GetRequiredService<ICroniqClient>(),
            sp,
            sp.GetRequiredService<ILoggerFactory>(),
            sp.GetRequiredService<CroniqHandlerRegistry>(),
            sp.GetRequiredService<RunnerIdentityResolver>(),
            sp.GetRequiredService<RunnerStateProbe>(),
            sp.GetService<TimeProvider>()));
        services.AddHostedService<CroniqRunnerHostedService>();
    }
}
