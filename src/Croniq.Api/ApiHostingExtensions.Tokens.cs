using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapTokenEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/tenants/{tenantId}/tokens", async (
            string tenantId,
            string? environment,
            IssueTokenRequest request,
            HttpContext httpContext,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ICroniqTokenIssuer tokenIssuer,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            return await IssueTokenAsync(
                    tenantId,
                    environment,
                    routeClientId: null,
                    request,
                    httpContext,
                    apiKeyStore,
                    tokenIssuer,
                    logger,
                    cancellationToken)
                .ConfigureAwait(false);
        })
        .WithDocs("Tokens_Issue_Tenant", "Issue tenant token", "Mints a Croniq-signed bearer token for the specified client (tenant-level variant).")
        .RequireCroniqTokenIssueScopes(CroniqScopes.ApiKeysManage);

        app.MapPost("/tenants/{tenantId}/api-clients/{clientId}/tokens", async (
            string tenantId,
            string clientId,
            string? environment,
            IssueTokenRequest request,
            HttpContext httpContext,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IApiKeyStore apiKeyStore,
            [FromServices] ICroniqTokenIssuer tokenIssuer,
            [FromServices] ILogger<ApiKeyAdminApiMarker> logger,
            CancellationToken cancellationToken) =>
        {
            return await IssueTokenAsync(
                    tenantId,
                    environment,
                    clientId,
                    request,
                    httpContext,
                    apiKeyStore,
                    tokenIssuer,
                    logger,
                    cancellationToken)
                .ConfigureAwait(false);
        })
        .WithDocs("Tokens_Issue_Client", "Issue client token", "Same payload as the tenant route but infers the clientId from the path.")
        .RequireCroniqTokenIssueScopes(CroniqScopes.ApiKeysManage);
    }

    private static async Task<IResult> IssueTokenAsync(
        string tenantId,
        string? environment,
        string? routeClientId,
        IssueTokenRequest request,
        HttpContext httpContext,
        IApiKeyStore apiKeyStore,
        ICroniqTokenIssuer tokenIssuer,
        ILogger<ApiKeyAdminApiMarker> logger,
        CancellationToken cancellationToken)
    {
        var clientId = routeClientId ?? request.ClientId;
        if (string.IsNullOrWhiteSpace(clientId))
        {
            return Results.BadRequest(new { error = "client-required", message = "ClientId is required." });
        }

        if (request.TtlMinutes.HasValue && request.TtlMinutes.Value <= 0)
        {
            return Results.BadRequest(new { error = "invalid-ttl", message = "TtlMinutes must be greater than zero." });
        }

        ApiClientDescriptor? client = null;
        if (httpContext.Items.TryGetValue(typeof(ApiClientDescriptor), out var cached)
            && cached is ApiClientDescriptor cachedClient
            && string.Equals(cachedClient.TenantId, tenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(cachedClient.ClientId, clientId, StringComparison.OrdinalIgnoreCase))
        {
            client = cachedClient;
        }

        client ??= await apiKeyStore.GetClientAsync(tenantId, clientId, cancellationToken).ConfigureAwait(false);
        if (client is null)
        {
            return Results.NotFound(new { error = "api-client-not-found", clientId });
        }

        if (!client.IsActive)
        {
            return Results.BadRequest(new { error = "client-inactive", message = "Inactive API clients cannot issue tokens." });
        }

        if (!string.IsNullOrWhiteSpace(environment)
            && !string.IsNullOrWhiteSpace(client.EnvironmentTag)
            && !string.Equals(client.EnvironmentTag, environment, StringComparison.OrdinalIgnoreCase))
        {
            return Results.BadRequest(new { error = "environment-mismatch", message = "Client environment does not match the requested environment." });
        }

        var guardEnvironment = environment ?? client.EnvironmentTag;

        var allowedScopes = client.Scopes ?? Array.Empty<string>();
        var requestedScopes = NormalizeScopes(request.Scopes);
        var tokenScopes = requestedScopes.Count == 0 ? allowedScopes : requestedScopes;
        if (tokenScopes.Count == 0)
        {
            return Results.BadRequest(new { error = "missing-scopes", message = "Assign scopes to the client before issuing tokens." });
        }

        if (!AreScopesAllowed(tokenScopes, allowedScopes))
        {
            return Results.BadRequest(new { error = "invalid-scopes", message = "Requested scopes must be a subset of the client scopes." });
        }

        TimeSpan? lifetime = null;
        if (request.TtlMinutes.HasValue)
        {
            lifetime = TimeSpan.FromMinutes(request.TtlMinutes.Value);
        }

        try
        {
            var token = await tokenIssuer.IssueAsync(new CroniqTokenIssueRequest(
                tenantId,
                clientId,
                guardEnvironment,
                tokenScopes,
                request.Audience,
                lifetime), cancellationToken).ConfigureAwait(false);

            return Results.Ok(new IssueTokenResponse(token.AccessToken, token.TokenType, token.ExpiresInSeconds));
        }
        catch (Exception ex)
        {
            logger.LogError(ex, "failed to issue token for tenant {TenantId} client {ClientId}", tenantId, clientId);
            return Results.Problem(statusCode: StatusCodes.Status500InternalServerError, title: "token-issue-failed", detail: ex.Message);
        }
    }

    private static IReadOnlyCollection<string> NormalizeScopes(IReadOnlyCollection<string>? requestedScopes)
    {
        if (requestedScopes is null || requestedScopes.Count == 0)
        {
            return Array.Empty<string>();
        }

        var normalized = requestedScopes
            .Where(scope => !string.IsNullOrWhiteSpace(scope))
            .Select(scope => scope.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return normalized.Length == 0 ? Array.Empty<string>() : normalized;
    }

    private static bool AreScopesAllowed(IReadOnlyCollection<string> requested, IReadOnlyCollection<string> allowed)
    {
        if (requested.Count == 0)
        {
            return true;
        }

        if (allowed.Count == 0)
        {
            return false;
        }

        var permitted = new HashSet<string>(allowed, StringComparer.OrdinalIgnoreCase);
        return requested.All(permitted.Contains);
    }
}
