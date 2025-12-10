using System.Reflection;
using Croniq.Webhooks;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqWebhookPersistence(builder.Configuration);
builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

builder.Services.AddCroniqWebhookObservability(
    builder.Configuration,
    builder.Logging,
    options => options.ServiceVersion = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "dev");

var app = builder.Build();

app.UseCroniqWebhooks();

app.Run();
