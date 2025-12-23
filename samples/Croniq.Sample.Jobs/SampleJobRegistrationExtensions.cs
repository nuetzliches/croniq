using Croniq;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Sample.Jobs;

public static class SampleJobRegistrationExtensions
{
    public static IServiceCollection AddCroniqSampleJobs(this IServiceCollection services)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));

        services
            .AddCroniqJob<LoggingSampleJob>()
            .AddTrigger("0/5 * * * * ?", trigger =>
            {
                trigger.TriggerId = "samples-log-every-5s";
                trigger.ManagedBy = "Croniq.Sample.Jobs";
                trigger.StartAtUtc = DateTimeOffset.UtcNow.AddSeconds(10);
                trigger.Metadata = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                {
                    ["seededBy"] = "Croniq.Sample.Jobs"
                };
            })
            .AddTrigger("@once", trigger =>
            {
                trigger.TriggerId = "samples-log-once";
                trigger.ManagedBy = "Croniq.Sample.Jobs";
                trigger.StartAtUtc = DateTimeOffset.UtcNow;
                trigger.Metadata = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
                {
                    ["seededBy"] = "Croniq.Sample.Jobs",
                    ["runType"] = "once"
                };
            });

        return services;
    }
}
