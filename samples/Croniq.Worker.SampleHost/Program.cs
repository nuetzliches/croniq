using Croniq.Core;
using Croniq.Core.Options;
using Croniq.JobStore.InMemory;
using Croniq.Providers.Default;
using Croniq.Persistence.Xtraq;
using Croniq.SampleJobs;
using Croniq.Worker.SampleHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

var builder = Host.CreateApplicationBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.Configure<CroniqOptions>(builder.Configuration.GetSection("Croniq:Core"));
builder.Services.Configure<XtraqSharedOptions>(builder.Configuration.GetSection("Croniq:Xtraq"));

builder.Services.AddCroniqDefaultProviders();
builder.Services.AddCroniqCore();
builder.Services.AddCroniqSampleJobs();

ConfigurePersistence(builder);

builder.Services.AddHostedService<CroniqWorkerHostedService>();
builder.Services.AddLogging(logging =>
{
    logging.SetMinimumLevel(LogLevel.Information);
    logging.AddSimpleConsole(options =>
    {
        options.SingleLine = true;
        options.TimestampFormat = "HH:mm:ss ";
    });
});

await builder.Build().RunAsync();

static void ConfigurePersistence(HostApplicationBuilder builder)
{
    var mode = builder.Configuration["Croniq:Persistence:Mode"] ?? "InMemory";
    if (string.Equals(mode, "Xtraq", StringComparison.OrdinalIgnoreCase))
    {
        var connection = builder.Configuration["Croniq:Persistence:Xtraq:ConnectionString"]
            ?? builder.Configuration["Croniq:Xtraq:ConnectionString"];
        if (string.IsNullOrWhiteSpace(connection))
        {
            throw new InvalidOperationException("Croniq:Xtraq:ConnectionString is required when Persistence.Mode = Xtraq.");
        }

        builder.Services.AddCroniqXtraqPersistence(opts =>
        {
            opts.ConnectionString = connection;
        });
    }
    else
    {
        builder.Services.AddCroniqInMemoryJobStore();
    }
}
