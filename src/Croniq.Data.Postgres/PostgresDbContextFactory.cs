using System;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;
using Npgsql.EntityFrameworkCore.PostgreSQL;

namespace Croniq.Data.Postgres;

/// <summary>
/// Provides design-time creation for Entity Framework tooling.
/// </summary>
public sealed class PostgresDbContextFactory : IDesignTimeDbContextFactory<PostgresDbContext>
{
    private const string DefaultConnection =
        "Host=localhost;Port=5432;Database=croniq_design;Username=postgres;Password=postgres";

    public PostgresDbContext CreateDbContext(string[] args)
    {
        var connectionString = Environment.GetEnvironmentVariable("CRONIQ_POSTGRES_CONNECTION") ?? DefaultConnection;

        var builder = new DbContextOptionsBuilder<PostgresDbContext>();
        builder.UseNpgsql(connectionString, sql => sql.EnableRetryOnFailure());
        builder.EnableSensitiveDataLogging();
        builder.EnableDetailedErrors();

        return new PostgresDbContext(builder.Options);
    }
}
