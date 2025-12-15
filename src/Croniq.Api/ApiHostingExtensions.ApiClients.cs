using System;
using System.Linq;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.AspNetCore.Mvc;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapApiClientEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapGet("/tenants/{tenantId}/api-clients", async (
            string tenantId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var clients = await apiKeyStore.ListClientsAsync(tenantId, environment, cancellationToken).ConfigureAwait(false);
            var payload = clients.Select(ToApiClientResponse).ToArray();
            return Results.Ok(payload);
        })
        .WithDocs("ApiClients_List", "List API clients", "Returns all registered API clients for the tenant, optionally filtered by environment.");

        app.MapPost("/tenants/{tenantId}/api-clients", async (
            string tenantId,
            string? environment,
            UpsertApiClientRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(request.ClientId))
            {
                return Results.BadRequest(new { error = "client-required", message = "ClientId is required." });
            }

            if (!string.IsNullOrWhiteSpace(environment)
                && !string.IsNullOrWhiteSpace(request.EnvironmentTag)
                && !string.Equals(environment, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            {
                return Results.BadRequest(new { error = "environment-mismatch", message = "Body environmentTag must match the query parameter value." });
            }

            var effectiveEnvironment = request.EnvironmentTag ?? environment;
            var scopes = NormalizeScopes(request.Scopes);
            var isActive = request.IsActive ?? true;

            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, effectiveEnvironment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var upsert = new ApiClientUpsertRequest(
                tenantId,
                request.ClientId,
                request.Name,
                effectiveEnvironment,
                scopes,
                isActive);

            var descriptor = await apiKeyStore.UpsertClientAsync(upsert, cancellationToken).ConfigureAwait(false);
            return Results.Ok(ToApiClientResponse(descriptor));
        })
        .WithDocs("ApiClients_Upsert", "Create or update API client", "Creates a tenant-scoped API client or updates metadata/scopes when the client already exists.");

        app.MapDelete("/tenants/{tenantId}/api-clients/{clientId}", async (
            string tenantId,
            string clientId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var deleted = await apiKeyStore.DeleteClientAsync(tenantId, clientId, cancellationToken).ConfigureAwait(false);
            if (!deleted)
            {
                return Results.NotFound(new { error = "api-client-not-found", clientId });
            }

            return Results.NoContent();
        })
        .WithDocs("ApiClients_Delete", "Delete API client", "Deletes the API client metadata and revokes any associated API keys.");

        app.MapGet("/tenants/{tenantId}/api-clients/{clientId}", async (
            string tenantId,
            string clientId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var authFailure = WebhookAuthorization.Ensure(callerContextAccessor, tenantId, environment, CroniqScopes.ApiKeysManage);
            if (authFailure is not null)
            {
                return authFailure;
            }

            var client = await apiKeyStore.GetClientAsync(tenantId, clientId, cancellationToken).ConfigureAwait(false);
            if (client is null)
            {
                return Results.NotFound(new { error = "api-client-not-found", clientId });
            }

            return Results.Ok(ToApiClientResponse(client));
        })
        .WithDocs("ApiClients_Get", "Get API client", "Returns metadata about a tenant-scoped API client, including scopes and activity flags.");
    }
}
