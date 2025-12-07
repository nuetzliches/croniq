using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;
using Croniq.Persistence.Xtraq.Health;

namespace Croniq.Persistence.Xtraq;

/// <summary>
/// DI helpers to wire the Xtraq-backed persistence provider.
/// </summary>
public static class DependencyInjectionExtensions
{
    /// <summary>
    /// Registers the Xtraq provider using code-based options configuration.
    /// </summary>
    public static IServiceCollection AddCroniqXtraqPersistence(
        this IServiceCollection services,
        Action<XtraqOptions> configureOptions,
        Action<ParameterBindingOptions>? configureBindings = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configureOptions is null) throw new ArgumentNullException(nameof(configureOptions));

        services.Configure(configureOptions);

        services.AddSingleton<XtraqDbContextOptions>(sp =>
        {
            var opts = sp.GetRequiredService<IOptions<XtraqOptions>>().Value
                ?? throw new InvalidOperationException("XtraqOptions are required.");
            if (string.IsNullOrWhiteSpace(opts.ConnectionString))
            {
                throw new InvalidOperationException("XtraqOptions.ConnectionString must be provided.");
            }

            var dbOptions = new XtraqDbContextOptions
            {
                ConnectionString = opts.ConnectionString,
                CommandTimeout = 30
            };

            // Bind the actor table-valued parameter so all procs requiring @Actor get it automatically.
            dbOptions.ParameterBindings.BindTable(
                "@Actor",
                (_, ct) => ValueTask.FromResult<IEnumerable<Core.ActorRef>>(
                    [Core.ActorRef.Create(opts.Actor)]
                ));

            configureBindings?.Invoke(dbOptions.ParameterBindings);
            return dbOptions;
        });

        services.AddSingleton<XtraqDbContext>();
        services.AddSingleton<IJobPersistenceProvider, XtraqJobPersistenceProvider>();
        services.AddSingleton<IPersistenceHealth, XtraqPersistenceHealth>();

        return services;
    }

    /// <summary>
    /// Registers the Xtraq provider using configuration binding (e.g. appsettings section).
    /// </summary>
    public static IServiceCollection AddCroniqXtraqPersistence(
        this IServiceCollection services,
        IConfiguration configuration,
        string sectionName = "Croniq:Xtraq",
        Action<ParameterBindingOptions>? configureBindings = null)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configuration is null) throw new ArgumentNullException(nameof(configuration));

        var section = configuration.GetSection(sectionName);
        var bound = section.Get<XtraqOptions>() ?? new XtraqOptions();

        return services.AddCroniqXtraqPersistence(opts =>
        {
            opts.ConnectionString = bound.ConnectionString ?? opts.ConnectionString;
            opts.Schema = bound.Schema;
        }, configureBindings);
    }
}
