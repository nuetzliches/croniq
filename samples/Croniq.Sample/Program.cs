using Croniq;
using Croniq.Sample;
using Croniq.Sample.Jobs;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

var builder = Host.CreateApplicationBuilder(args);

builder.Logging.AddSimpleConsole(options =>
{
    options.SingleLine = true;
    options.TimestampFormat = "HH:mm:ss ";
});

builder.Services.AddCroniq();
builder.Services.AddCroniqSampleJobs();
builder.Services.AddHostedService<SampleTriggerSeedHostedService>();

await builder.Build().RunAsync();
