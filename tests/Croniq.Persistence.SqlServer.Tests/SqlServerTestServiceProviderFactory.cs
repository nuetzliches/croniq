using System;
using Croniq.Persistence.SqlServer;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Persistence.SqlServer.Tests;

internal static class SqlServerTestServiceProviderFactory
{
    public static ServiceProvider Create(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqSqlServerPersistence(sql =>
        {
            sql.ConnectionString = connectionString;
            var verboseEf = TestLogging.EnableVerboseEfDiagnostics();
            sql.EnableDetailedErrors = verboseEf;
            sql.EnableSensitiveDataLogging = verboseEf;
            sql.SuppressMarsSavepointWarning = true;
        });

        return services.BuildServiceProvider();
    }
}
