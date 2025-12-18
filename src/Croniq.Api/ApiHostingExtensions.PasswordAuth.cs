using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Auth.SqlServer;
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

            var auth = services.GetService<PasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var tenants = services.GetService<ITenantStore>();
            if (tenants is null)
            {
                return Results.NotFound();
            }

            var resolvedTenantId = await ResolveTenantIdAsync(
                    tenants,
                    request.TenantReference,
                    options.CurrentValue,
                    cancellationToken)
                .ConfigureAwait(false);

            if (string.IsNullOrWhiteSpace(resolvedTenantId))
            {
                // Do not leak whether the tenant exists.
                return Results.Unauthorized();
            }

            var result = await auth.LoginAsync(
                    resolvedTenantId,
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

            var tenantDescriptor = await tenants.GetByIdAsync(resolvedTenantId, cancellationToken).ConfigureAwait(false);

            return Results.Ok(new
            {
                tenantReference = tenantDescriptor?.Reference,
                accessToken = result.AccessToken,
                tokenType = "Bearer",
                expiresIn = result.ExpiresInSeconds,
                refreshToken = result.RefreshToken,
                passwordChangeRequired = result.PasswordChangeRequired
            });
        })
        .WithDocs("Auth_Login", "Password login", "Authenticates a username/password and issues access + refresh tokens. Tenant can be provided via tenantReference; it can be omitted if a default tenant is configured.");

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

            var auth = services.GetService<PasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var tenants = services.GetService<ITenantStore>();
            if (tenants is null)
            {
                return Results.NotFound();
            }

            var resolvedTenantId = await ResolveTenantIdAsync(
                    tenants,
                    request.TenantReference,
                    options.CurrentValue,
                    cancellationToken)
                .ConfigureAwait(false);

            if (string.IsNullOrWhiteSpace(resolvedTenantId))
            {
                // Do not leak whether the tenant exists.
                return Results.Unauthorized();
            }

            var result = await auth.RefreshAsync(
                    resolvedTenantId,
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

            return Results.Ok(new
            {
                accessToken = result.AccessToken,
                tokenType = "Bearer",
                expiresIn = result.ExpiresInSeconds,
                refreshToken = result.RefreshToken,
                passwordChangeRequired = result.PasswordChangeRequired
            });
        })
        .WithDocs("Auth_Refresh", "Refresh access token", "Rotates the refresh token and returns a new access token.");

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

            var auth = services.GetService<PasswordAuthService>();
            if (auth is null)
            {
                return Results.NotFound();
            }

            var tenants = services.GetService<ITenantStore>();
            if (tenants is null)
            {
                return Results.NotFound();
            }

            var resolvedTenantId = await ResolveTenantIdAsync(
                    tenants,
                    request.TenantReference,
                    options.CurrentValue,
                    cancellationToken)
                .ConfigureAwait(false);

            if (string.IsNullOrWhiteSpace(resolvedTenantId))
            {
                // Do not leak whether the tenant exists.
                return Results.Unauthorized();
            }

            var revoked = await auth.LogoutAsync(resolvedTenantId, request.RefreshToken, cancellationToken).ConfigureAwait(false);
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
        .WithDocs("Auth_Logout", "Logout", "Revokes the provided refresh token.");

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

            var auth = services.GetService<PasswordAuthService>();
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
        .WithDocs("Auth_ChangePassword", "Change password", "Changes the password for the currently authenticated password user. Requires a valid access token.");
    }

    private static async Task<string?> ResolveTenantIdAsync(
        ITenantStore tenants,
        string? tenantReference,
        PasswordAuthOptions? options,
        CancellationToken cancellationToken)
    {
        if (tenants is null) throw new ArgumentNullException(nameof(tenants));

        static string? Clean(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();

        tenantReference = Clean(tenantReference);

        if (tenantReference is not null)
        {
            var byRef = await tenants.GetByReferenceAsync(tenantReference, cancellationToken).ConfigureAwait(false);
            if (byRef is not null && byRef.IsActive)
            {
                return byRef.TenantId;
            }
        }

        var defaultTenant = Clean(options?.DefaultTenant);
        if (defaultTenant is not null)
        {
            var byRef = await tenants.GetByReferenceAsync(defaultTenant, cancellationToken).ConfigureAwait(false);
            if (byRef is not null && byRef.IsActive)
            {
                return byRef.TenantId;
            }
        }

        return null;
    }
}
