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
        services.AddLogging(builder => builder.AddSimpleConsole());
        services.AddCroniqSqlServerPersistence(sql =>
        {
            sql.ConnectionString = connectionString;
            sql.EnableDetailedErrors = true;
            sql.EnableSensitiveDataLogging = true;
        });

        return services.BuildServiceProvider();
    }
}
