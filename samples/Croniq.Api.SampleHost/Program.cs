using Croniq.Api;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration.AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true);

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

var app = builder.Build();

app.UseCroniqApi();

app.Run();
