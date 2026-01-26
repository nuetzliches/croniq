using Microsoft.AspNetCore.Builder;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    public static IServiceCollection AddCroniqApiCors(
        this IServiceCollection services,
        IConfiguration configuration,
        IHostEnvironment environment)
    {
        if (services is null)
        {
            throw new ArgumentNullException(nameof(services));
        }

        if (configuration is null)
        {
            throw new ArgumentNullException(nameof(configuration));
        }

        if (environment is null)
        {
            throw new ArgumentNullException(nameof(environment));
        }

        var allowedOrigins = configuration
            .GetSection("Croniq:Api:Cors:AllowedOrigins")
            .Get<string[]>() ?? Array.Empty<string>();

        services.AddCors(options =>
        {
            options.AddPolicy(CorsPolicyName, policy =>
            {
                if (allowedOrigins.Length == 0)
                {
                    if (environment.IsDevelopment())
                    {
                        policy.AllowAnyOrigin().AllowAnyHeader().AllowAnyMethod();
                        return;
                    }

                    throw new InvalidOperationException("Croniq:Api:Cors:AllowedOrigins must be configured outside Development.");
                }

                policy
                    .WithOrigins(allowedOrigins)
                    .AllowAnyHeader()
                    .AllowAnyMethod();
            });
        });

        return services;
    }

    public static WebApplication UseCroniqApiCors(this WebApplication app)
    {
        if (app is null)
        {
            throw new ArgumentNullException(nameof(app));
        }

        app.UseCors(CorsPolicyName);
        return app;
    }
}
