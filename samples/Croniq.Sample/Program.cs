using Croniq;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

var builder = Host.CreateApplicationBuilder(args);

builder.Logging.AddSimpleConsole(options =>
{
    options.SingleLine = true;
    options.TimestampFormat = "HH:mm:ss ";
});

builder.Services
    .AddCroniq()
    .AddCroniqJob("samples", "smoke", (context, _) =>
    {
        var metadataCount = context.Metadata?.Count ?? 0;
        context.Logger.LogInformation(
            "Executing Croniq smoke job for {JobKey} with {MetadataCount} metadata entries",
            context.JobKey,
            metadataCount);
        return Task.CompletedTask;
    })
    .AddTrigger("0/5 * * * * ?", trigger =>
    {
        trigger.TriggerId = "samples-smoke-every-5s";
        trigger.ManagedBy = "Croniq.Sample";
        trigger.StartAtUtc = DateTimeOffset.UtcNow;
        trigger.Metadata = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["seededBy"] = "Croniq.Sample"
        };
    });

await builder.Build().RunAsync();
