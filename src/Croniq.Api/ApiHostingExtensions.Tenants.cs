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
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

            if (request is null || string.IsNullOrWhiteSpace(request.Reference) || string.IsNullOrWhiteSpace(request.Name))
            {
                return Results.BadRequest(new { error = "invalid-request", message = "Reference and name are required." });
            }

            var descriptor = await tenantStore.CreateAsync(request.Reference, request.Name, cancellationToken).ConfigureAwait(false);
            return Results.Created($"/tenants/{descriptor.TenantId}", ToTenantResponse(descriptor));
        })
        .WithDocs("Tenants_Create", "Create tenant", "Creates or updates a tenant record based on the provided reference and name.");

        app.MapGet("/tenants", async (
            string? state,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

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
        .WithDocs("Tenants_List", "List tenants", "Returns tenant metadata. Use state=all to include inactive tenants.");

        app.MapGet("/tenants/{tenantId}", async (
            string tenantId,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

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
        .WithDocs("Tenants_Get", "Get tenant", "Returns tenant metadata for the provided tenant identifier.");

        app.MapDelete("/tenants/{tenantId}", async (
            string tenantId,
            [FromServices] ITenantStore tenantStore,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            CancellationToken cancellationToken) =>
        {
            var authFailure = TenantGuard.EnsureAdminScopes(callerContextAccessor, CroniqScopes.TenantsAdmin);
            if (authFailure is not null)
            {
                return authFailure;
            }

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
        .WithDocs("Tenants_Deactivate", "Deactivate tenant", "Marks the tenant as inactive without deleting historical data.");
    }
}
