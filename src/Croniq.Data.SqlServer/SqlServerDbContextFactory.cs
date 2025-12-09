using System;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;

namespace Croniq.Data.SqlServer;

/// <summary>
/// Provides design-time creation for Entity Framework tooling.
/// </summary>
public sealed class SqlServerDbContextFactory : IDesignTimeDbContextFactory<SqlServerDbContext>
{
    private const string DefaultConnection =
        "Server=(localdb)\\mssqllocaldb;Database=Croniq.DesignTime;Trusted_Connection=True;TrustServerCertificate=True";

    public SqlServerDbContext CreateDbContext(string[] args)
    {
        var connectionString = Environment.GetEnvironmentVariable("CRONIQ_SQL_CONNECTION") ?? DefaultConnection;

        var builder = new DbContextOptionsBuilder<SqlServerDbContext>();
        builder.UseSqlServer(connectionString, sql => sql.EnableRetryOnFailure());
        builder.EnableSensitiveDataLogging();
        builder.EnableDetailedErrors();

        return new SqlServerDbContext(builder.Options);
    }
}
