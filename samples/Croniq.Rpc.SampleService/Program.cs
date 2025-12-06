using Microsoft.Extensions.Configuration;

var configuration = new ConfigurationBuilder()
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables()
    .Build();

var endpoint = configuration["Croniq:Endpoint"] ?? "http://localhost:5000";
var apiKey = configuration["Croniq:ApiKey"] ?? "dev-key";

Console.WriteLine("RPC sample client placeholder.");
Console.WriteLine($"Endpoint: {endpoint}");
Console.WriteLine($"API Key : {apiKey}");
// TODO: replace with real Croniq RPC client once generated types are available.
