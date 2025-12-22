using Croniq;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Sample.Jobs;

public static class SampleJobRegistrationExtensions
{
    public static IServiceCollection AddCroniqSampleJobs(this IServiceCollection services)
    {
        if (services is null) throw new ArgumentNullException(nameof(services));
        services.AddCroniqJob<LoggingSampleJob>();
        return services;
    }
}
