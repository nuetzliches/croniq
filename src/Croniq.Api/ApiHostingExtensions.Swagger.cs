using System;
using System.Collections.Generic;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.OpenApi;
using Swashbuckle.AspNetCore.SwaggerGen;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    public sealed record CroniqSwaggerOptions
    {
        public string DocumentName { get; init; } = "v1";

        public string Title { get; init; } = "Croniq Scheduler API";

        public string Version { get; init; } = "v1";

        public bool AddApiKeyScheme { get; init; } = true;

        public string ApiKeySchemeName { get; init; } = "X-Croniq-Key";

        public string ApiKeyHeaderName { get; init; } = "X-Croniq-Key";

        public bool AddBearerScheme { get; init; } = true;

        public string BearerSchemeName { get; init; } = "Bearer";
    }

    public static IServiceCollection AddCroniqApiSwagger(
        this IServiceCollection services,
        Action<CroniqSwaggerOptions>? configure = null)
    {
        if (services is null)
        {
            throw new ArgumentNullException(nameof(services));
        }

        services.AddEndpointsApiExplorer();

        var swaggerOptions = new CroniqSwaggerOptions();
        configure?.Invoke(swaggerOptions);

        services.AddSwaggerGen(options => ConfigureSwagger(options, swaggerOptions));
        return services;
    }

    private static void ConfigureSwagger(SwaggerGenOptions options, CroniqSwaggerOptions swaggerOptions)
    {
        options.SwaggerDoc(
            swaggerOptions.DocumentName,
            new OpenApiInfo { Title = swaggerOptions.Title, Version = swaggerOptions.Version });

        OpenApiSecuritySchemeReference? apiKeyReference = null;
        if (swaggerOptions.AddApiKeyScheme)
        {
            options.AddSecurityDefinition(swaggerOptions.ApiKeySchemeName, new OpenApiSecurityScheme
            {
                Description = "Croniq API key passed via X-Croniq-Key header.",
                Name = swaggerOptions.ApiKeyHeaderName,
                In = ParameterLocation.Header,
                Type = SecuritySchemeType.ApiKey
            });

            apiKeyReference = new OpenApiSecuritySchemeReference(
                referenceId: swaggerOptions.ApiKeySchemeName,
                hostDocument: null,
                externalResource: null);
        }

        OpenApiSecuritySchemeReference? bearerReference = null;
        if (swaggerOptions.AddBearerScheme)
        {
            options.AddSecurityDefinition(swaggerOptions.BearerSchemeName, new OpenApiSecurityScheme
            {
                Description = "Croniq bearer token passed via Authorization: Bearer {token}.",
                Name = "Authorization",
                In = ParameterLocation.Header,
                Type = SecuritySchemeType.Http,
                Scheme = "bearer",
                BearerFormat = "JWT"
            });

            bearerReference = new OpenApiSecuritySchemeReference(
                referenceId: swaggerOptions.BearerSchemeName,
                hostDocument: null,
                externalResource: null);
        }

        if (apiKeyReference is not null)
        {
            options.AddSecurityRequirement(_ => new OpenApiSecurityRequirement
            {
                { apiKeyReference, new List<string>() }
            });
        }

        if (bearerReference is not null)
        {
            options.AddSecurityRequirement(_ => new OpenApiSecurityRequirement
            {
                { bearerReference, new List<string>() }
            });
        }
    }
}
