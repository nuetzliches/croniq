using Croniq.DbMigrator;
using Microsoft.Data.SqlClient;

var connectionString = Environment.GetEnvironmentVariable("CRONIQ_SQL_CONNECTION");
if (string.IsNullOrWhiteSpace(connectionString))
{
    Console.Error.WriteLine("CRONIQ_SQL_CONNECTION environment variable is required.");
    return 1;
}

var cancellation = new CancellationTokenSource(TimeSpan.FromMinutes(10));
var token = cancellation.Token;
var builder = new SqlConnectionStringBuilder(connectionString);
if (string.IsNullOrWhiteSpace(builder.InitialCatalog))
{
    Console.Error.WriteLine("Connection string must include Initial Catalog/Database.");
    return 1;
}

var database = builder.InitialCatalog;
var scriptRoot = Path.Combine(AppContext.BaseDirectory, "sql");
if (!Directory.Exists(scriptRoot))
{
    Console.Error.WriteLine($"SQL script folder not found: {scriptRoot}");
    return 1;
}

Console.WriteLine($"Applying Croniq schema to database '{database}'...");
await EnsureDatabaseExistsAsync(builder, token).ConfigureAwait(false);

var scripts = ScriptManifest.SqlScripts;
await using var connection = new SqlConnection(builder.ConnectionString);
await connection.OpenAsync(token).ConfigureAwait(false);
foreach (var script in scripts)
{
    var filePath = Path.Combine(scriptRoot, script.Replace('/', Path.DirectorySeparatorChar));
    Console.WriteLine($"Executing {script}...");
    await SqlScriptBatchExecutor.ExecuteAsync(connection, filePath, token).ConfigureAwait(false);
}

Console.WriteLine("Croniq database schema applied successfully.");
return 0;

static async Task EnsureDatabaseExistsAsync(SqlConnectionStringBuilder builder, CancellationToken cancellationToken)
{
    var database = builder.InitialCatalog;
    var masterBuilder = new SqlConnectionStringBuilder(builder.ConnectionString)
    {
        InitialCatalog = "master"
    };

    await using var connection = new SqlConnection(masterBuilder.ConnectionString);
    await connection.OpenAsync(cancellationToken).ConfigureAwait(false);
    await using var command = connection.CreateCommand();
    command.CommandText = $"IF DB_ID('{database.Replace("'", "''")}') IS NULL CREATE DATABASE [{database.Replace("]", "]]")}];";
    await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
}
