using System;
using Grpc.AspNetCore.Server;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Routing;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    public static IServiceCollection AddCroniqGrpcReflection(this IServiceCollection services)
    {
        if (services is null)
        {
            throw new ArgumentNullException(nameof(services));
        }

        services.AddGrpcReflection();
        return services;
    }

    public static IEndpointConventionBuilder MapCroniqGrpcReflection(this IEndpointRouteBuilder endpoints)
    {
        if (endpoints is null)
        {
            throw new ArgumentNullException(nameof(endpoints));
        }

        return endpoints.MapGrpcReflectionService();
    }
}
