using System;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Runner;

public static class RunnerServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqRunner(
        this IServiceCollection services,
        Action<CroniqRunnerOptions> configure)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        if (configure is null) throw new ArgumentNullException(nameof(configure));

        var options = new CroniqRunnerOptions();
        configure(options);

        if (options.Config is null)
        {
            throw new InvalidOperationException("CroniqRunnerOptions.Config must be set.");
        }
        if (options.Handlers.Count == 0)
        {
            throw new InvalidOperationException("At least one onExecute handler must be registered.");
        }

        services.AddSingleton(options);
        services.AddSingleton(sp =>
        {
            var loggerFactory = sp.GetService<ILoggerFactory>();
            var config = options.Config!;
            if (config.Logger is null && loggerFactory is not null)
            {
                config = config with { Logger = loggerFactory.CreateLogger("Croniq.Runner") };
            }

            var runner = new CroniqRunner(config);
            foreach (var handler in options.Handlers)
            {
                runner.OnExecute(handler.Key, handler.Value);
            }
            return runner;
        });

        return services;
    }

    public static IServiceCollection AddCroniqRunnerHostedService(
        this IServiceCollection services,
        Action<CroniqRunnerOptions> configure)
    {
        services.AddCroniqRunner(configure);
        services.AddHostedService<CroniqRunnerHostedService>();
        return services;
    }

    public static IServiceCollection AddCroniqRunnerHostedService(this IServiceCollection services)
    {
        services.AddHostedService<CroniqRunnerHostedService>();
        return services;
    }
}
