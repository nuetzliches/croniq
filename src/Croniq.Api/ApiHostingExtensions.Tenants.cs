using System;
using System.Linq;
using System.Threading;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapTenantAdminEndpoints(WebApplication app)
    {
        app.MapPost("/tenants", async (
            UpsertTenantRequest request,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            if (request is null || string.IsNullOrWhiteSpace(request.TenantId) || string.IsNullOrWhiteSpace(request.Name))
            {
                return Results.BadRequest(new { error = "invalid-request", message = "TenantId and Name are required." });
            }

            var descriptor = await tenantStore.CreateAsync(new TenantCreateRequest(request.Name, request.TenantId.Trim()), cancellationToken).ConfigureAwait(false);
            return Results.Created($"/tenants/{descriptor.TenantId}", ToTenantResponse(descriptor));
        })
        .WithDocs("Tenants_Create", "Create tenant", "Creates or updates a tenant record for the provided tenantId.")
        .Produces<TenantResponse>(StatusCodes.Status201Created)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqAdminScopes(CroniqScopes.TenantsAdmin);

        app.MapGet("/tenants", async (
            string? state,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var normalizedState = string.IsNullOrWhiteSpace(state) ? "active" : state.Trim();
            if (!string.Equals(normalizedState, "active", StringComparison.OrdinalIgnoreCase)
                && !string.Equals(normalizedState, "all", StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "invalid-state", message = "state must be 'active' or 'all'." });
            }

            var tenants = await tenantStore.ListAsync(cancellationToken).ConfigureAwait(false);
            var filtered = string.Equals(normalizedState, "all", StringComparison.OrdinalIgnoreCase)
                ? tenants
                : tenants.Where(t => t.IsActive).ToArray();
            var payload = filtered.Select(ToTenantResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("Tenants_List", "List tenants", "Returns tenant metadata. Use state=all to include inactive tenants.")
        .Produces<TenantResponse[]>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqAdminScopes(CroniqScopes.TenantsAdmin);

        app.MapGet("/tenants/{tenantId}", async (
            string tenantId,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "TenantId is required." });
            }

            var descriptor = await tenantStore.GetByIdAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (descriptor is null)
            {
                return Results.NotFound(new { error = "tenant-not-found", tenantId });
            }

            return Results.Ok(ToTenantResponse(descriptor));
        })
        .WithDocs("Tenants_Get", "Get tenant", "Returns tenant metadata for the provided tenant identifier.")
        .Produces<TenantResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqAdminScopes(CroniqScopes.TenantsAdmin);

        app.MapDelete("/tenants/{tenantId}", async (
            string tenantId,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(tenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "TenantId is required." });
            }

            var deactivated = await tenantStore.DeactivateAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (!deactivated)
            {
                return Results.NotFound(new { error = "tenant-not-found", tenantId });
            }

            return Results.NoContent();
        })
        .WithDocs("Tenants_Deactivate", "Deactivate tenant", "Marks the tenant as inactive without deleting historical data.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqAdminScopes(CroniqScopes.TenantsAdmin);
    }
}
