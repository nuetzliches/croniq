using System;
using System.Linq;
using Microsoft.AspNetCore.Builder;
using Microsoft.Extensions.Configuration;
using Swashbuckle.AspNetCore.SwaggerUI;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    public sealed record CroniqSwaggerUiOptions
    {
        public string DocumentName { get; init; } = "v1";

        public string DisplayName { get; init; } = "Croniq Scheduler API v1";

        public bool DisplayRequestDuration { get; init; } = true;

        public bool MapGrpcReflectionService { get; init; } = true;

        public bool LogStartupMessage { get; init; } = true;

        public string FallbackBaseAddress { get; init; } = "http://localhost:5080";
    }

    public static WebApplication UseCroniqApiSwaggerUi(
        this WebApplication app,
        IConfiguration configuration,
        Action<CroniqSwaggerUiOptions>? configure = null)
    {
        if (app is null)
        {
            throw new ArgumentNullException(nameof(app));
        }

        if (configuration is null)
        {
            throw new ArgumentNullException(nameof(configuration));
        }

        var swaggerEnabled = app.Environment.IsDevelopment()
            || configuration.GetValue<bool>("Croniq:Api:ExposeSchemas");

        var options = new CroniqSwaggerUiOptions();
        configure?.Invoke(options);

        if (options.LogStartupMessage)
        {
            var addresses = app.Urls?.Any() == true ? string.Join(", ", app.Urls) : options.FallbackBaseAddress;

            if (swaggerEnabled)
            {
                var swaggerAddress = app.Urls?.FirstOrDefault() ?? options.FallbackBaseAddress;
                app.Logger.LogInformation(
                    "Croniq API listening on {Addresses}. Swagger UI: {SwaggerUrl}",
                    addresses,
                    $"{swaggerAddress}/swagger");
            }
            else
            {
                app.Logger.LogInformation("Croniq API listening on {Addresses}. Swagger UI disabled.", addresses);
            }
        }

        if (!swaggerEnabled)
        {
            return app;
        }

        app.UseSwagger();
        app.UseSwaggerUI(ui => ConfigureSwaggerUi(ui, options));

        if (options.MapGrpcReflectionService)
        {
            _ = app.MapCroniqGrpcReflection();
        }

        return app;
    }

    private static void ConfigureSwaggerUi(SwaggerUIOptions ui, CroniqSwaggerUiOptions options)
    {
        ui.SwaggerEndpoint($"/swagger/{options.DocumentName}/swagger.json", options.DisplayName);

        if (options.DisplayRequestDuration)
        {
            ui.DisplayRequestDuration();
        }
    }
}
