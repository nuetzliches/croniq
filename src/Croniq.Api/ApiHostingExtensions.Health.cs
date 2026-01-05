using System;
using System.Threading;
using Croniq.Api.Models;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapHealthEndpoints(WebApplication app)
    {
        app.MapGet("/health", () => Results.Ok(new HealthStatusResponse("ok")))
            .WithDocs("Health_Get", "Health probe", "Returns 200 when the Croniq API process is alive.")
            .Produces<HealthStatusResponse>(StatusCodes.Status200OK);

        app.MapGet("/health/persistence", async ([FromServices] IServiceProvider sp, CancellationToken ct) =>
        {
            var provider = sp.GetService<IJobPersistenceProvider>();
            var providerName = provider?.GetType().FullName ?? "unknown";

            var health = sp.GetService<IPersistenceHealth>();
            if (health is null)
            {
                return Results.Ok(new PersistenceHealthResponse("ok", providerName, "no-db-provider-configured", null));
            }

            try
            {
                var result = await health.CheckAsync(ct).ConfigureAwait(false);
                if (result.IsHealthy)
                {
                    return Results.Ok(new PersistenceHealthResponse("ok", providerName, null, "reachable"));
                }

                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unhealthy", detail: result.Detail);
            }
            catch (Exception ex)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unreachable", detail: ex.Message);
            }
        })
        .WithDocs("Health_Persistence_Get", "Persistence health", "Checks the configured job persistence provider for reachability.")
        .Produces<PersistenceHealthResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status503ServiceUnavailable);
    }
}
