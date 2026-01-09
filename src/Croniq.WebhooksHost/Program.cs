using Croniq.Hosting;
using Croniq.Webhooks;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

builder.Services.AddCroniqWebhookObservability(builder.Configuration, builder.Logging);

builder.Services.AddCroniqJobsFromConfiguration(builder.Configuration);

var app = builder.Build();

app.UseCroniqWebhooks();

app.Run();
