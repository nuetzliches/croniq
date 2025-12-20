using System;
using System.Threading;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapHealthEndpoints(WebApplication app)
    {
        app.MapGet("/health", () => Results.Ok(new { status = "ok" }))
            .WithDocs("Health_Get", "Health probe", "Returns 200 when the Croniq API process is alive.");

        app.MapGet("/health/persistence", async ([FromServices] IServiceProvider sp, CancellationToken ct) =>
        {
            var provider = sp.GetService<IJobPersistenceProvider>();
            var providerName = provider?.GetType().FullName ?? "unknown";

            var health = sp.GetService<IPersistenceHealth>();
            if (health is null)
            {
                return Results.Ok(new { status = "ok", provider = providerName, note = "no-db-provider-configured" });
            }

            try
            {
                var result = await health.CheckAsync(ct).ConfigureAwait(false);
                if (result.IsHealthy)
                {
                    return Results.Ok(new { status = "ok", provider = providerName, db = "reachable" });
                }

                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unhealthy", detail: result.Detail);
            }
            catch (Exception ex)
            {
                return Results.Problem(statusCode: StatusCodes.Status503ServiceUnavailable, title: "db-unreachable", detail: ex.Message);
            }
        })
        .WithDocs("Health_Persistence_Get", "Persistence health", "Checks the configured job persistence provider for reachability.");
    }
}
