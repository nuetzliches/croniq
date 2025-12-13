using Croniq.JobStore.InMemory;
using Croniq.Persistence.SqlServer;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Sample.WorkerHost;

internal static class PersistenceServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqSamplePersistence(this IServiceCollection services, IConfiguration configuration)
    {
        var mode = configuration["Croniq:Persistence:Mode"] ?? "InMemory";
        if (string.Equals(mode, "SqlServer", StringComparison.OrdinalIgnoreCase))
        {
            var sqlSection = configuration.GetSection("Croniq:SqlServer");
            var connection = sqlSection["ConnectionString"];
            if (string.IsNullOrWhiteSpace(connection))
            {
                throw new InvalidOperationException("Croniq:SqlServer:ConnectionString is required when Persistence.Mode = SqlServer.");
            }

            var persistenceSection = configuration.GetSection("Croniq:Persistence:SqlServer");
            services.AddCroniqSqlServerPersistence(options =>
            {
                sqlSection.Bind(options);
                options.ConnectionString = connection;
            }, persistenceSection.Exists() ? persistence => persistenceSection.Bind(persistence) : null);
        }
        else
        {
            services.AddCroniqInMemoryJobStore();
        }

        return services;
    }
}
