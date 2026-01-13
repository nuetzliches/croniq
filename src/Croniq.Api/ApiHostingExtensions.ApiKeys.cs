using System;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Observability;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapApiKeyEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/tenants/{tenantId}/api-keys", async (
            string tenantId,
            IssueApiKeyRequest request,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            if (string.IsNullOrWhiteSpace(request.ClientId))
            {
                return Results.BadRequest(new { error = "client-required", message = "ClientId is required." });
            }

            if (request.TtlHours.HasValue && request.TtlHours.Value <= 0)
            {
                return Results.BadRequest(new { error = "invalid-ttl", message = "TtlHours must be greater than zero." });
            }

            var scopes = NormalizeScopes(request.Scopes);
            TimeSpan? ttl = request.TtlHours.HasValue ? TimeSpan.FromHours(request.TtlHours.Value) : null;
            var issueRequest = new ApiKeyIssueRequest(tenantId, request.ClientId, request.EnvironmentTag, scopes, ttl);

            try
            {
                var result = await apiKeyStore.IssueAsync(issueRequest, cancellationToken).ConfigureAwait(false);
                return Results.Ok(ToIssueApiKeyResponse(result));
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "failed to issue api key for tenant {TenantId} client {ClientId}", IdentifierHashing.HashTenantId(tenantId) ?? string.Empty, request.ClientId);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "api-key-issue-failed", detail: ex.Message);
            }
        })
        .WithDocs("ApiKeys_Issue", "Issue API key", "Creates a new API key for the specified tenant client and returns the plaintext once.")
        .Produces<IssueApiKeyResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status500InternalServerError)
        .RequireCroniqTenantScopeFromBody<IssueApiKeyRequest>(request => request.EnvironmentTag, CroniqScopes.ApiKeysManage);

        app.MapPost("/tenants/{tenantId}/api-keys/{keyId}/rotate", async (
            string tenantId,
            string keyId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            try
            {
                var result = await apiKeyStore.RotateAsync(tenantId, keyId, cancellationToken).ConfigureAwait(false);
                if (result is null)
                {
                    return Results.NotFound(new { error = "api-key-not-found", keyId });
                }

                return Results.Ok(ToIssueApiKeyResponse(result));
            }
            catch (Exception ex)
            {
                logger.LogError(ex, "failed to rotate api key {KeyId} for tenant {TenantId}", keyId, IdentifierHashing.HashTenantId(tenantId) ?? string.Empty);
                return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "api-key-rotation-failed", detail: ex.Message);
            }
        })
        .WithDocs("ApiKeys_Rotate", "Rotate API key", "Revokes an existing API key and returns a fresh secret for the same client.")
        .Produces<IssueApiKeyResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError)
        .RequireCroniqTenantScope(requireEnvironment: false, CroniqScopes.ApiKeysManage);

        app.MapDelete("/tenants/{tenantId}/api-keys/{keyId}", async (
            string tenantId,
            string keyId,
            string? environment,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            CancellationToken cancellationToken) =>
        {
            var revoked = await apiKeyStore.RevokeAsync(tenantId, keyId, cancellationToken).ConfigureAwait(false);
            if (!revoked)
            {
                return Results.NotFound(new { error = "api-key-not-found", keyId });
            }

            return Results.NoContent();
        })
        .WithDocs("ApiKeys_Delete", "Revoke API key", "Immediately revokes an API key for the tenant.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status404NotFound)
        .RequireCroniqTenantScope(requireEnvironment: false, CroniqScopes.ApiKeysManage);
    }
}
