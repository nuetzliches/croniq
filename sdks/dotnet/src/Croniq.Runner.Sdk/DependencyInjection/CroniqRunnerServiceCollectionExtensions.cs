using System.Linq;

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
/// <para>
/// <b>Idempotency:</b> calling any <c>AddCroniqRunner(...)</c> overload more
/// than once on the same <see cref="IServiceCollection"/> is safe — the
/// second and subsequent calls no-op for the shared setup (options bind,
/// HTTP client, auth handler, hosted service) and return a builder that
/// still allows further <c>AddCroniqJob&lt;T&gt;(...)</c> chaining. This
/// matches the common ASP.NET Core convention of <c>AddX(...)</c> being
/// safe to call from multiple feature modules.
/// </para>
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

        if (IsAlreadyRegistered(services))
        {
            return new CroniqRunnerBuilder(services);
        }

        services.AddSingleton<CroniqRunnerMarker>();

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

        if (IsAlreadyRegistered(services))
        {
            return new CroniqRunnerBuilder(services);
        }

        services.AddSingleton<CroniqRunnerMarker>();

        services
            .AddOptions<CroniqRunnerOptions>()
            .Bind(configurationSection)
            .ValidateDataAnnotations()
            .ValidateOnStart();

        RegisterCore(services);
        return new CroniqRunnerBuilder(services);
    }

    private static bool IsAlreadyRegistered(IServiceCollection services) =>
        services.Any(d => d.ServiceType == typeof(CroniqRunnerMarker));

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

    // Sentinel: presence in the service collection means a previous
    // AddCroniqRunner(...) has already wired up the shared infrastructure
    // (options bind, HTTP client, auth handler, hosted service). Subsequent
    // calls must NOT redo any of that — Bind would duplicate IList-typed
    // options (Capabilities, Tags) and a second AddHttpMessageHandler would
    // put two auth handlers in the pipeline, which appends a second value
    // to the Authorization header → server reads a comma-joined string and
    // returns 401.
    private sealed class CroniqRunnerMarker;
}
