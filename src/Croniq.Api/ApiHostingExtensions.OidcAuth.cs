using Croniq.Api.Security;
using Microsoft.AspNetCore.Mvc;
using Microsoft.AspNetCore.WebUtilities;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private static void MapOidcAuthEndpoints(WebApplication app)
    {
        _ = app ?? throw new ArgumentNullException(nameof(app));

        app.MapGet("/auth/oidc/start", async (
            [FromQuery] string? returnUrl,
            HttpContext context,
            [FromServices] OidcLoginService oidcLogin,
            CancellationToken cancellationToken) =>
        {
            if (!oidcLogin.TryResolveOptions(out var oidcOptions, out var loginOptions, out var error))
            {
                return error!;
            }

            var authorizationEndpoint = await oidcLogin.GetAuthorizationEndpointAsync(oidcOptions, cancellationToken).ConfigureAwait(false);
            if (authorizationEndpoint is null)
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status500InternalServerError,
                    title: "oidc-authorization-missing",
                    detail: "OIDC metadata missing authorization endpoint.");
            }

            var state = oidcLogin.CreateState(context, returnUrl, loginOptions);
            var codeChallenge = OidcLoginService.CreateCodeChallenge(state.CodeVerifier);
            var scopes = loginOptions.Scopes?.Length > 0
                ? string.Join(' ', loginOptions.Scopes)
                : "openid profile offline_access";

            var redirect = QueryHelpers.AddQueryString(authorizationEndpoint.ToString(), new Dictionary<string, string?>
            {
                ["client_id"] = loginOptions.ClientId,
                ["redirect_uri"] = loginOptions.RedirectUri,
                ["response_type"] = "code",
                ["scope"] = scopes,
                ["state"] = state.State,
                ["code_challenge"] = codeChallenge,
                ["code_challenge_method"] = "S256"
            });

            return Results.Redirect(redirect);
        })
        .WithDocs("Auth_OidcStart", "OIDC login start", "Redirects to the configured OIDC provider for login.")
        .Produces(StatusCodes.Status302Found)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError);

        app.MapGet("/auth/oidc/callback", async (
            [FromQuery] string? code,
            [FromQuery] string? state,
            [FromQuery(Name = "error")] string? errorCode,
            [FromQuery(Name = "error_description")] string? errorDescription,
            HttpContext context,
            [FromServices] OidcLoginService oidcLogin,
            CancellationToken cancellationToken) =>
        {
            if (!oidcLogin.TryResolveOptions(out var oidcOptions, out var loginOptions, out var error))
            {
                return error!;
            }

            if (!string.IsNullOrWhiteSpace(errorCode))
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status401Unauthorized,
                    title: "oidc-login-failed",
                    detail: string.IsNullOrWhiteSpace(errorDescription) ? errorCode : errorDescription);
            }

            if (string.IsNullOrWhiteSpace(code))
            {
                return Results.BadRequest(new { error = "missing-code", message = "code is required." });
            }

            if (!oidcLogin.TryConsumeState(context, state, loginOptions, out var loginState, out var stateError))
            {
                return stateError!;
            }

            var tokenResponse = await oidcLogin.RedeemCodeAsync(
                oidcOptions,
                loginOptions,
                code,
                loginState.CodeVerifier,
                cancellationToken).ConfigureAwait(false);

            if (tokenResponse is null || string.IsNullOrWhiteSpace(tokenResponse.AccessToken))
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status401Unauthorized,
                    title: "oidc-token-failed",
                    detail: "OIDC token exchange failed.");
            }

            if (string.IsNullOrWhiteSpace(tokenResponse.RefreshToken))
            {
                return Results.Problem(
                    statusCode: StatusCodes.Status500InternalServerError,
                    title: "oidc-refresh-missing",
                    detail: "OIDC refresh token missing; ensure offline_access is configured.");
            }

            oidcLogin.SetRefreshCookie(context, tokenResponse.RefreshToken, loginOptions);
            oidcLogin.EnsureCsrfCookie(context, loginOptions);

            var callbackUrl = oidcLogin.BuildUiRedirectUrl(loginOptions, "/auth/oidc/callback");
            var redirect = QueryHelpers.AddQueryString(callbackUrl, "returnUrl", loginState.ReturnUrl);
            return Results.Redirect(redirect);
        })
        .WithDocs("Auth_OidcCallback", "OIDC login callback", "Handles the OIDC code exchange and sets the refresh cookie.")
        .Produces(StatusCodes.Status302Found)
        .Produces(StatusCodes.Status400BadRequest)
        .Produces(StatusCodes.Status401Unauthorized)
        .Produces(StatusCodes.Status404NotFound)
        .Produces(StatusCodes.Status500InternalServerError);
    }
}
