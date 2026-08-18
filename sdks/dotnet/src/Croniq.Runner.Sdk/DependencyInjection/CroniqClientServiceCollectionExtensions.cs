using System.Linq;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Internal;

using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.DependencyInjection;

/// <summary>
/// <c>IServiceCollection</c> extensions to register the producer-side
/// trigger client (<see cref="ICroniqTriggerClient"/>).
/// <para>
/// Independent of <c>AddCroniqRunner(...)</c>: an application can register
/// either or both. The trigger client gets its own <c>HttpClient</c>, its own
/// auth handler, and its own options (<see cref="CroniqClientOptions"/>,
/// section <c>Croniq:Client</c>) because triggering requires the
/// <c>jobs:trigger</c> (or <c>admin</c>) scope, which is distinct from the
/// runner's poll credentials.
/// </para>
/// <para>
/// <b>Idempotency:</b> calling any <c>AddCroniqClient(...)</c> overload more
/// than once on the same <see cref="IServiceCollection"/> is safe — the
/// second and subsequent calls no-op, matching <c>AddCroniqRunner</c>.
/// </para>
/// </summary>
public static class CroniqClientServiceCollectionExtensions
{
    /// <summary>
    /// Register the Croniq trigger client with explicit option configuration.
    /// </summary>
    public static IServiceCollection AddCroniqClient(
        this IServiceCollection services,
        Action<CroniqClientOptions>? configureOptions = null)
    {
        ArgumentNullException.ThrowIfNull(services);

        if (IsAlreadyRegistered(services))
        {
            return services;
        }

        services.AddSingleton<CroniqClientMarker>();

        var optionsBuilder = services
            .AddOptions<CroniqClientOptions>()
            .ValidateOnStart();

        if (configureOptions is not null)
        {
            optionsBuilder.Configure(configureOptions);
        }

        RegisterCore(services);
        return services;
    }

    /// <summary>
    /// Register the Croniq trigger client binding options from a
    /// configuration section. Use with
    /// <c>builder.Configuration.GetSection(CroniqClientOptions.SectionName)</c>.
    /// </summary>
    public static IServiceCollection AddCroniqClient(
        this IServiceCollection services,
        IConfiguration configurationSection)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configurationSection);

        if (IsAlreadyRegistered(services))
        {
            return services;
        }

        services.AddSingleton<CroniqClientMarker>();

        services
            .AddOptions<CroniqClientOptions>()
            .Bind(configurationSection)
            .ValidateOnStart();

        RegisterCore(services);
        return services;
    }

    private static bool IsAlreadyRegistered(IServiceCollection services) =>
        services.Any(d => d.ServiceType == typeof(CroniqClientMarker));

    private static void RegisterCore(IServiceCollection services)
    {
        // Source-generated DataAnnotations validation (trim/AOT-safe replacement
        // for the reflection-based ValidateDataAnnotations()). Driven by the
        // ValidateOnStart() registered in each public overload above.
        services.AddSingleton<IValidateOptions<CroniqClientOptions>, CroniqClientOptionsValidator>();

        // Transport security (#440): refuse a cleartext ServerUrl on a
        // non-loopback host, so the trigger credential can't be shipped in the
        // clear by an operator who only swapped the host in the default.
        services.AddSingleton<IValidateOptions<CroniqClientOptions>, CroniqClientOptionsSecurityValidator>();

        services.AddHttpClient<ICroniqTriggerClient, CroniqTriggerClient>((sp, http) =>
        {
            var opts = sp.GetRequiredService<IOptions<CroniqClientOptions>>().Value;
            http.BaseAddress = new Uri(opts.ServerUrl.TrimEnd('/'));
            // Per-request timeout is enforced via linked CancellationTokenSource
            // in CroniqTriggerClient (RequestTimeout option).
            http.Timeout = Timeout.InfiniteTimeSpan;
        })
        .AddHttpMessageHandler<CroniqClientAuthHandler>();

        services.TryAddTransient<CroniqClientAuthHandler>();
    }

    // Sentinel: presence in the service collection means a previous
    // AddCroniqClient(...) has already wired up the shared setup (options
    // bind, HTTP client, auth handler). Subsequent calls must NOT redo any
    // of that — a second AddHttpMessageHandler would put two auth handlers
    // in the pipeline (see CroniqRunnerMarker for the failure mode).
    private sealed class CroniqClientMarker;
}
