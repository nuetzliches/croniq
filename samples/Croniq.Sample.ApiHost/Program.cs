using Croniq.Api;
using Croniq.Sample.Jobs;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqSampleJobs();
builder.Services.AddCroniqApiRateLimiter();

var app = builder.Build();

app.UseCroniqApi();

app.Run();
