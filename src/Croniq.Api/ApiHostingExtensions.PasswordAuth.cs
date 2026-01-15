using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Options;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapPasswordAuthEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapPost("/auth/login", async (
            PasswordLoginRequest request,
            [FromServices] IServiceProvider services,
            [FromServices] IOptionsMonitor<PasswordAuthOptions> options,
            CancellationToken cancellationToken) =>
        {
            if (!(options.CurrentValue?.Enabled ?? false))
            {
                return Results.NotFound();
            }

            var auth = services.GetService<IPasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var tenants = services.GetService<ITenantStore>();
            if (tenants is null)
            {
                return Results.NotFound();
            }

            if (string.IsNullOrWhiteSpace(request.TenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "tenantId is required." });
            }

            var tenantId = request.TenantId.Trim();
            var tenant = await tenants.GetByIdAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (tenant is null || !tenant.IsActive)
            {
                // Do not leak whether the tenant exists.
                return Results.Unauthorized();
            }

            var result = await auth.LoginAsync(
                    tenant.TenantId,
                    request.Username,
                    request.Password,
                    request.EnvironmentTag,
                    request.Scopes,
                    request.Audience,
                    cancellationToken)
                .ConfigureAwait(false);

            if (result is null)
            {
                return Results.NotFound();
            }

            if (!result.Success)
            {
                if (result.LockoutEndUtc.HasValue)
                {
                    return Results.Problem(
                        title: "locked",
                        detail: "too many failed login attempts",
                        statusCode: StatusCodes.Status403Forbidden,
                        extensions: new Dictionary<string, object?> { ["lockoutEndUtc"] = result.LockoutEndUtc });
                }

                return Results.Unauthorized();
            }

            if (string.IsNullOrWhiteSpace(result.AccessToken) || string.IsNullOrWhiteSpace(result.RefreshToken))
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status500InternalServerError,
                    title: "auth-token-missing",
                    detail: "Authentication token response was incomplete.");
            }

            return Results.Ok(new PasswordAuthResponse(
                tenant.TenantId,
                result.AccessToken,
                "Bearer",
                result.ExpiresInSeconds,
                result.RefreshToken,
                result.PasswordChangeRequired));
        })
        .WithDocs("Auth_Login", "Password login", "Authenticates a username/password and issues access + refresh tokens. Tenant must be provided via tenantId.")
        .Produces<PasswordAuthResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status401Unauthorized)
        .Produces(StatusCodes.Status403Forbidden)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError);

        app.MapPost("/auth/refresh", async (
            PasswordRefreshRequest request,
            [FromServices] IServiceProvider services,
            [FromServices] IOptionsMonitor<PasswordAuthOptions> options,
            CancellationToken cancellationToken) =>
        {
            if (!(options.CurrentValue?.Enabled ?? false))
            {
                return Results.NotFound();
            }

            var auth = services.GetService<IPasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var tenants = services.GetService<ITenantStore>();
            if (tenants is null)
            {
                return Results.NotFound();
            }

            if (string.IsNullOrWhiteSpace(request.TenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "tenantId is required." });
            }

            var tenantId = request.TenantId.Trim();
            var tenant = await tenants.GetByIdAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (tenant is null || !tenant.IsActive)
            {
                // Do not leak whether the tenant exists.
                return Results.Unauthorized();
            }

            var result = await auth.RefreshAsync(
                    tenant.TenantId,
                    request.RefreshToken,
                    request.EnvironmentTag,
                    request.Scopes,
                    request.Audience,
                    cancellationToken)
                .ConfigureAwait(false);

            if (result is null)
            {
                return Results.NotFound();
            }

            if (!result.Success)
            {
                return Results.Unauthorized();
            }

            if (string.IsNullOrWhiteSpace(result.AccessToken) || string.IsNullOrWhiteSpace(result.RefreshToken))
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status500InternalServerError,
                    title: "auth-token-missing",
                    detail: "Authentication token response was incomplete.");
            }

            return Results.Ok(new PasswordAuthResponse(
                tenant.TenantId,
                result.AccessToken,
                "Bearer",
                result.ExpiresInSeconds,
                result.RefreshToken,
                result.PasswordChangeRequired));
        })
        .WithDocs("Auth_Refresh", "Refresh access token", "Rotates the refresh token and returns a new access token.")
        .Produces<PasswordAuthResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status401Unauthorized)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError);

        app.MapPost("/auth/logout", async (
            PasswordLogoutRequest request,
            [FromServices] IServiceProvider services,
            [FromServices] IOptionsMonitor<PasswordAuthOptions> options,
            CancellationToken cancellationToken) =>
        {
            if (!(options.CurrentValue?.Enabled ?? false))
            {
                return Results.NotFound();
            }

            var auth = services.GetService<IPasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var tenants = services.GetService<ITenantStore>();
            if (tenants is null)
            {
                return Results.NotFound();
            }

            if (string.IsNullOrWhiteSpace(request.TenantId))
            {
                return Results.BadRequest(new { error = "missing-tenant", message = "tenantId is required." });
            }

            var tenantId = request.TenantId.Trim();
            var tenant = await tenants.GetByIdAsync(tenantId, cancellationToken).ConfigureAwait(false);
            if (tenant is null || !tenant.IsActive)
            {
                // Do not leak whether the tenant exists.
                return Results.Unauthorized();
            }

            var revoked = await auth.LogoutAsync(tenant.TenantId, request.RefreshToken, cancellationToken).ConfigureAwait(false);
            if (revoked is null)
            {
                return Results.NotFound();
            }

            if (revoked == false)
            {
                return Results.NoContent();
            }

            return Results.NoContent();
        })
        .WithDocs("Auth_Logout", "Logout", "Revokes the provided refresh token.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status401Unauthorized)
        .Produces(StatusCodes.Status404NotFound);

        app.MapPost("/auth/change-password", async (
            PasswordChangePasswordRequest request,
            [FromServices] IServiceProvider services,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IOptionsMonitor<PasswordAuthOptions> options,
            CancellationToken cancellationToken) =>
        {
            if (!(options.CurrentValue?.Enabled ?? false))
            {
                return Results.NotFound();
            }

            var auth = services.GetService<IPasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var caller = callerContextAccessor.Current;
            if (caller is null || !caller.IsActive)
            {
                return Results.Unauthorized();
            }

            if (caller.CallerType != CallerType.User)
            {
                return Results.Problem(statusCode: StatusCodes.Status403Forbidden, title: "forbidden");
            }

            var changed = await auth.ChangePasswordAsync(
                    caller.TenantId,
                    caller.CallerId,
                    request.CurrentPassword,
                    request.NewPassword,
                    cancellationToken)
                .ConfigureAwait(false);

            if (changed is null)
            {
                return Results.NotFound();
            }

            if (!changed.Value)
            {
                return Results.Unauthorized();
            }

            return Results.NoContent();
        })
        .WithDocs("Auth_ChangePassword", "Change password", "Changes the password for the currently authenticated password user. Requires a valid access token.")
        .Produces(StatusCodes.Status204NoContent)
        .Produces(StatusCodes.Status401Unauthorized)
        .Produces(StatusCodes.Status403Forbidden)
        .Produces(StatusCodes.Status404NotFound)
        .AllowPasswordChangeRequired()
        .RequireCroniqCaller();
    }

    // Tenant resolution is explicit: callers must provide tenantId.
}
