using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.OpenApi;
using Swashbuckle.AspNetCore.SwaggerGen;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    public sealed record CroniqSwaggerOptions
    {
        public string DocumentName { get; init; } = "v1";

        public string Title { get; init; } = "Croniq API";

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

        options.OperationFilter<AnonymousPathsOperationFilter>();

        if (swaggerOptions.AddApiKeyScheme)
        {
            options.AddSecurityDefinition(swaggerOptions.ApiKeySchemeName, new OpenApiSecurityScheme
            {
                Description = "Croniq API key passed via X-Croniq-Key header.",
                Name = swaggerOptions.ApiKeyHeaderName,
                In = ParameterLocation.Header,
                Type = SecuritySchemeType.ApiKey
            });
        }

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
        }

        if (swaggerOptions.AddApiKeyScheme)
        {
            options.AddSecurityRequirement(hostDocument => new OpenApiSecurityRequirement
            {
                {
                    new OpenApiSecuritySchemeReference(
                        referenceId: swaggerOptions.ApiKeySchemeName,
                        hostDocument: hostDocument,
                        externalResource: null),
                    new List<string>()
                }
            });
        }

        if (swaggerOptions.AddBearerScheme)
        {
            options.AddSecurityRequirement(hostDocument => new OpenApiSecurityRequirement
            {
                {
                    new OpenApiSecuritySchemeReference(
                        referenceId: swaggerOptions.BearerSchemeName,
                        hostDocument: hostDocument,
                        externalResource: null),
                    new List<string>()
                }
            });
        }
    }

    private sealed class AnonymousPathsOperationFilter : IOperationFilter
    {
        private static readonly string[] AnonymousPrefixes =
        [
            "health",
            "webhooks",
            "auth/login",
            "auth/oidc",
            "auth/refresh",
            "auth/logout",
        ];

        public void Apply(OpenApiOperation operation, OperationFilterContext context)
        {
            var relativePath = context.ApiDescription.RelativePath;
            if (string.IsNullOrWhiteSpace(relativePath))
            {
                return;
            }

            var normalized = relativePath.TrimStart('/');
            if (!AnonymousPrefixes.Any(prefix => normalized.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)))
            {
                return;
            }

            operation.Security = new List<OpenApiSecurityRequirement>();
        }
    }
}
