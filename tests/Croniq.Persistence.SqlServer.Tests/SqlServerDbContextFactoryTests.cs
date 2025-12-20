using System;
using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

public class SqlServerDbContextFactoryTests
{
    [Fact]
    public void CreateDbContext_UsesEnvironmentConnectionString()
    {
        const string key = "CRONIQ_SQL_CONNECTION";
        var original = Environment.GetEnvironmentVariable(key);
        var expected = "Server=tests;Database=Croniq;Trusted_Connection=True;Encrypt=False";

        try
        {
            Environment.SetEnvironmentVariable(key, expected);
            var factory = new SqlServerDbContextFactory();

            using var context = factory.CreateDbContext(Array.Empty<string>());
            var connectionString = context.Database.GetDbConnection().ConnectionString;

            connectionString.ShouldContain("Data Source=tests");
            connectionString.ShouldContain("Initial Catalog=Croniq");
            connectionString.ShouldContain("Encrypt=False");
        }
        finally
        {
            Environment.SetEnvironmentVariable(key, original);
        }
    }
}
