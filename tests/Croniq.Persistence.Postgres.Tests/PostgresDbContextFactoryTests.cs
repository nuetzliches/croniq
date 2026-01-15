using System;
using Croniq.Data.Postgres;
using Microsoft.EntityFrameworkCore;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.Postgres.Tests;

public class PostgresDbContextFactoryTests
{
    [Fact]
    public void CreateDbContext_UsesEnvironmentConnectionString()
    {
        const string key = "CRONIQ_POSTGRES_CONNECTION";
        var original = Environment.GetEnvironmentVariable(key);
        var expected = "Host=tests;Database=Croniq;Username=postgres;Password=secret";

        try
        {
            Environment.SetEnvironmentVariable(key, expected);
            var factory = new PostgresDbContextFactory();

            using var context = factory.CreateDbContext(Array.Empty<string>());
            var connectionString = context.Database.GetDbConnection().ConnectionString;

            connectionString.ShouldContain("Host=tests");
            connectionString.ShouldContain("Database=Croniq");
        }
        finally
        {
            Environment.SetEnvironmentVariable(key, original);
        }
    }
}


