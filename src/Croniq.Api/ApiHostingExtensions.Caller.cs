using System;
using Croniq.Auth.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapCallerEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapGet("/me", ([FromServices] ICallerContextAccessor callerContextAccessor) =>
        {
            var caller = callerContextAccessor.Current;
            if (caller is null || !caller.IsActive)
            {
                return Results.Unauthorized();
            }

            return Results.Ok(ToCallerInfoResponse(caller));
        })
        .WithDocs("Caller_Get", "Inspect caller", "Returns the current caller context (tenant, environment, scopes) after authentication.");
    }
}
