using System;
using Croniq.Persistence.Postgres;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Persistence.Postgres.Tests;

internal static class PostgresTestServiceProviderFactory
{
    public static ServiceProvider Create(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqPostgresPersistence(pg =>
        {
            pg.ConnectionString = connectionString;
            var verboseEf = TestLogging.EnableVerboseEfDiagnostics();
            pg.EnableDetailedErrors = verboseEf;
            pg.EnableSensitiveDataLogging = verboseEf;
        });

        return services.BuildServiceProvider();
    }
}


