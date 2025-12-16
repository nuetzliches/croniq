using System;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    public sealed record CroniqApiSchemasOptions
    {
        public Action<CroniqSwaggerOptions>? Swagger { get; init; }

        public bool AddGrpcReflection { get; init; } = true;
    }

    public static IServiceCollection AddCroniqApiSchemas(
        this IServiceCollection services,
        Action<CroniqApiSchemasOptions>? configure = null)
    {
        if (services is null)
        {
            throw new ArgumentNullException(nameof(services));
        }

        var options = new CroniqApiSchemasOptions();
        configure?.Invoke(options);

        services.AddCroniqApiSwagger(options.Swagger);

        if (options.AddGrpcReflection)
        {
            services.AddCroniqGrpcReflection();
        }

        return services;
    }
}
