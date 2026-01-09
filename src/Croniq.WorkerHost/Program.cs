using Croniq.Core;
using Croniq.Hosting;
using Microsoft.Extensions.Hosting;

var builder = Host.CreateApplicationBuilder(args);

builder.Services.AddCroniqWorkerServices(builder.Configuration);

builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Worker");

builder.Services.AddCroniqJobsFromConfiguration(builder.Configuration);

await builder.Build().RunAsync();
