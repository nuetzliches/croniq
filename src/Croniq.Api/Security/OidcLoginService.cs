using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.AspNetCore.DataProtection;
using Microsoft.Extensions.Options;
using Microsoft.IdentityModel.Protocols;
using Microsoft.IdentityModel.Protocols.OpenIdConnect;

namespace Croniq.Api.Security;

internal sealed class OidcLoginService
{
    internal const string RefreshCookieName = "croniq.oidc.refresh";
    internal const string CsrfCookieName = "croniq.oidc.csrf";
    internal const string StateCookieName = "croniq.oidc.state";
    internal const string CsrfHeaderName = "X-CSRF";

    private const string StateProtectorPurpose = "Croniq.Oidc.LoginState";
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);

    private readonly IDataProtector _stateProtector;
    private readonly IOptionsMonitor<CroniqOidcOptions> _oidcOptions;
    private readonly IOptionsMonitor<CroniqOidcLoginOptions> _loginOptions;
    private readonly IHttpClientFactory _httpClientFactory;
    private readonly ILogger<OidcLoginService> _logger;
    private readonly object _configurationLock = new();
    private ConfigurationManager<OpenIdConnectConfiguration>? _configurationManager;
    private string? _configuredAuthority;

    public OidcLoginService(
        IDataProtectionProvider dataProtectionProvider,
        IOptionsMonitor<CroniqOidcOptions> oidcOptions,
        IOptionsMonitor<CroniqOidcLoginOptions> loginOptions,
        IHttpClientFactory httpClientFactory,
        ILogger<OidcLoginService> logger)
    {
        _stateProtector = dataProtectionProvider.CreateProtector(StateProtectorPurpose);
        _oidcOptions = oidcOptions;
        _loginOptions = loginOptions;
        _httpClientFactory = httpClientFactory;
        _logger = logger;
    }

    public bool IsEnabled => _loginOptions.CurrentValue?.Enabled ?? false;

    public bool TryResolveOptions(out CroniqOidcOptions oidcOptions, out CroniqOidcLoginOptions loginOptions, out IResult? error)
    {
        oidcOptions = _oidcOptions.CurrentValue ?? new CroniqOidcOptions();
        loginOptions = _loginOptions.CurrentValue ?? new CroniqOidcLoginOptions();

        if (!loginOptions.Enabled)
        {
            error = Results.NotFound();
            return false;
        }

        if (!oidcOptions.Enabled)
        {
            error = Results.Problem(
                statusCode: StatusCodes.Status500InternalServerError,
                title: "oidc-disabled",
                detail: "OIDC bearer validation must be enabled for OIDC login.");
            return false;
        }

        if (string.IsNullOrWhiteSpace(oidcOptions.Authority))
        {
            error = Results.Problem(
                statusCode: StatusCodes.Status500InternalServerError,
                title: "oidc-authority-missing",
                detail: "Croniq:Auth:Oidc:Authority is required for OIDC login.");
            return false;
        }

        if (string.IsNullOrWhiteSpace(loginOptions.ClientId))
        {
            error = Results.Problem(
                statusCode: StatusCodes.Status500InternalServerError,
                title: "oidc-client-missing",
                detail: "Croniq:Auth:OidcLogin:ClientId is required for OIDC login.");
            return false;
        }

        if (string.IsNullOrWhiteSpace(loginOptions.RedirectUri))
        {
            error = Results.Problem(
                statusCode: StatusCodes.Status500InternalServerError,
                title: "oidc-redirect-missing",
                detail: "Croniq:Auth:OidcLogin:RedirectUri is required for OIDC login.");
            return false;
        }

        if (string.IsNullOrWhiteSpace(loginOptions.UiBaseUrl) || !Uri.TryCreate(loginOptions.UiBaseUrl, UriKind.Absolute, out _))
        {
            error = Results.Problem(
                statusCode: StatusCodes.Status500InternalServerError,
                title: "oidc-ui-base-missing",
                detail: "Croniq:Auth:OidcLogin:UiBaseUrl must be an absolute URL.");
            return false;
        }

        error = null;
        return true;
    }

    public async Task<Uri?> GetAuthorizationEndpointAsync(CroniqOidcOptions options, CancellationToken cancellationToken)
    {
        var configuration = await GetConfigurationAsync(options, cancellationToken).ConfigureAwait(false);
        if (string.IsNullOrWhiteSpace(configuration.AuthorizationEndpoint))
        {
            _logger.LogError("OIDC metadata missing authorization endpoint.");
            return null;
        }

        return new Uri(configuration.AuthorizationEndpoint);
    }

    public async Task<Uri?> GetTokenEndpointAsync(CroniqOidcOptions options, CancellationToken cancellationToken)
    {
        var configuration = await GetConfigurationAsync(options, cancellationToken).ConfigureAwait(false);
        if (string.IsNullOrWhiteSpace(configuration.TokenEndpoint))
        {
            _logger.LogError("OIDC metadata missing token endpoint.");
            return null;
        }

        return new Uri(configuration.TokenEndpoint);
    }

    public OidcLoginState CreateState(HttpContext context, string? returnUrl, CroniqOidcLoginOptions loginOptions)
    {
        var state = CreateSecureToken();
        var codeVerifier = CreateCodeVerifier();
        var resolvedReturnUrl = NormalizeReturnUrl(returnUrl, loginOptions);
        var issuedAt = DateTimeOffset.UtcNow;

        var payload = new OidcLoginState(state, codeVerifier, resolvedReturnUrl, issuedAt);
        var protectedState = _stateProtector.Protect(JsonSerializer.Serialize(payload, JsonOptions));

        var cookieOptions = CreateStateCookieOptions(loginOptions, issuedAt);
        context.Response.Cookies.Append(StateCookieName, protectedState, cookieOptions);

        return payload;
    }

    public bool TryConsumeState(
        HttpContext context,
        string? stateParam,
        CroniqOidcLoginOptions loginOptions,
        out OidcLoginState state,
        out IResult? error)
    {
        state = default!;
        error = null;

        if (string.IsNullOrWhiteSpace(stateParam))
        {
            error = Results.BadRequest(new { error = "missing-state", message = "state is required." });
            return false;
        }

        if (!context.Request.Cookies.TryGetValue(StateCookieName, out var rawState) || string.IsNullOrWhiteSpace(rawState))
        {
            error = Results.BadRequest(new { error = "missing-state-cookie", message = "OIDC state cookie is missing." });
            return false;
        }

        try
        {
            var json = _stateProtector.Unprotect(rawState);
            var parsed = JsonSerializer.Deserialize<OidcLoginState>(json, JsonOptions);
            if (parsed is null)
            {
                error = Results.BadRequest(new { error = "invalid-state", message = "OIDC state payload is invalid." });
                return false;
            }

            var maxAge = loginOptions.StateTtlMinutes <= 0 ? 5 : loginOptions.StateTtlMinutes;
            var expiresAt = parsed.IssuedAtUtc.AddMinutes(maxAge);
            if (DateTimeOffset.UtcNow > expiresAt)
            {
                error = Results.BadRequest(new { error = "state-expired", message = "OIDC state has expired." });
                return false;
            }

            if (!string.Equals(parsed.State, stateParam, StringComparison.Ordinal))
            {
                error = Results.BadRequest(new { error = "state-mismatch", message = "OIDC state does not match." });
                return false;
            }

            state = parsed;
            return true;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "Failed to decode OIDC state.");
            error = Results.BadRequest(new { error = "invalid-state", message = "OIDC state could not be validated." });
            return false;
        }
        finally
        {
            ClearStateCookie(context, loginOptions);
        }
    }

    public void SetRefreshCookie(HttpContext context, string refreshToken, CroniqOidcLoginOptions loginOptions)
    {
        var options = CreateRefreshCookieOptions(loginOptions);
        context.Response.Cookies.Append(RefreshCookieName, refreshToken, options);
    }

    public void ClearRefreshCookies(HttpContext context, CroniqOidcLoginOptions loginOptions)
    {
        var options = CreateRefreshCookieOptions(loginOptions);
        context.Response.Cookies.Delete(RefreshCookieName, options);
        context.Response.Cookies.Delete(CsrfCookieName, CreateCsrfCookieOptions(loginOptions));
    }

    public bool TryGetRefreshToken(HttpContext context, out string refreshToken)
    {
        refreshToken = string.Empty;
        if (!context.Request.Cookies.TryGetValue(RefreshCookieName, out var raw) || string.IsNullOrWhiteSpace(raw))
        {
            return false;
        }

        refreshToken = raw;
        return true;
    }

    public string EnsureCsrfCookie(HttpContext context, CroniqOidcLoginOptions loginOptions)
    {
        if (context.Request.Cookies.TryGetValue(CsrfCookieName, out var existing) && !string.IsNullOrWhiteSpace(existing))
        {
            return existing;
        }

        var token = CreateSecureToken();
        context.Response.Cookies.Append(CsrfCookieName, token, CreateCsrfCookieOptions(loginOptions));
        return token;
    }

    public bool ValidateCsrf(HttpContext context, CroniqOidcLoginOptions loginOptions, out IResult? error)
    {
        error = null;
        if (!context.Request.Cookies.TryGetValue(CsrfCookieName, out var cookieToken) || string.IsNullOrWhiteSpace(cookieToken))
        {
            error = Results.StatusCode(StatusCodes.Status403Forbidden);
            return false;
        }

        var headerToken = context.Request.Headers[CsrfHeaderName].FirstOrDefault();
        if (string.IsNullOrWhiteSpace(headerToken) || !string.Equals(cookieToken, headerToken, StringComparison.Ordinal))
        {
            error = Results.StatusCode(StatusCodes.Status403Forbidden);
            return false;
        }

        return true;
    }

    public async Task<OidcTokenResponse?> RedeemCodeAsync(
        CroniqOidcOptions oidcOptions,
        CroniqOidcLoginOptions loginOptions,
        string code,
        string codeVerifier,
        CancellationToken cancellationToken)
    {
        var tokenEndpoint = await GetTokenEndpointAsync(oidcOptions, cancellationToken).ConfigureAwait(false);
        if (tokenEndpoint is null)
        {
            return null;
        }

        var payload = new Dictionary<string, string>
        {
            ["grant_type"] = "authorization_code",
            ["code"] = code,
            ["client_id"] = loginOptions.ClientId,
            ["redirect_uri"] = loginOptions.RedirectUri,
            ["code_verifier"] = codeVerifier
        };

        if (!string.IsNullOrWhiteSpace(loginOptions.ClientSecret))
        {
            payload["client_secret"] = loginOptions.ClientSecret!;
        }

        return await PostTokenRequestAsync(tokenEndpoint, payload, cancellationToken).ConfigureAwait(false);
    }

    public async Task<OidcTokenResponse?> RefreshAsync(
        CroniqOidcOptions oidcOptions,
        CroniqOidcLoginOptions loginOptions,
        string refreshToken,
        CancellationToken cancellationToken)
    {
        var tokenEndpoint = await GetTokenEndpointAsync(oidcOptions, cancellationToken).ConfigureAwait(false);
        if (tokenEndpoint is null)
        {
            return null;
        }

        var payload = new Dictionary<string, string>
        {
            ["grant_type"] = "refresh_token",
            ["refresh_token"] = refreshToken,
            ["client_id"] = loginOptions.ClientId
        };

        if (!string.IsNullOrWhiteSpace(loginOptions.ClientSecret))
        {
            payload["client_secret"] = loginOptions.ClientSecret!;
        }

        return await PostTokenRequestAsync(tokenEndpoint, payload, cancellationToken).ConfigureAwait(false);
    }

    public string BuildUiRedirectUrl(CroniqOidcLoginOptions loginOptions, string returnUrl)
    {
        var baseUri = new Uri(loginOptions.UiBaseUrl);
        var normalized = returnUrl.StartsWith('/') ? returnUrl : $"/{returnUrl}";
        var combined = new Uri(baseUri, normalized);
        return combined.ToString();
    }

    public string? ResolveTenantId(string accessToken, CroniqOidcOptions oidcOptions)
    {
        var payload = TryReadJwtPayload(accessToken);
        if (payload is null)
        {
            return null;
        }

        if (TryGetClaim(payload, oidcOptions.TenantClaim, out var value))
        {
            return value;
        }

        foreach (var claim in oidcOptions.TenantFallbackClaims ?? Array.Empty<string>())
        {
            if (TryGetClaim(payload, claim, out value))
            {
                return value;
            }
        }

        return null;
    }

    private async Task<OpenIdConnectConfiguration> GetConfigurationAsync(CroniqOidcOptions options, CancellationToken cancellationToken)
    {
        var authority = options.Authority?.TrimEnd('/');
        if (string.IsNullOrWhiteSpace(authority))
        {
            throw new InvalidOperationException("Croniq:Auth:Oidc:Authority is required for OIDC login.");
        }

        lock (_configurationLock)
        {
            if (_configurationManager is null || !string.Equals(_configuredAuthority, authority, StringComparison.OrdinalIgnoreCase))
            {
                var metadataAddress = string.IsNullOrWhiteSpace(options.MetadataAddress)
                    ? $"{authority}/.well-known/openid-configuration"
                    : options.MetadataAddress;

                var documentRetriever = new HttpDocumentRetriever
                {
                    RequireHttps = options.RequireHttpsMetadata
                };

                _configurationManager = new ConfigurationManager<OpenIdConnectConfiguration>(
                    metadataAddress!,
                    new OpenIdConnectConfigurationRetriever(),
                    documentRetriever)
                {
                    AutomaticRefreshInterval = options.MetadataRefreshInterval,
                    RefreshInterval = options.MetadataRefreshInterval
                };

                _configuredAuthority = authority;
            }
        }

        return await _configurationManager!.GetConfigurationAsync(cancellationToken).ConfigureAwait(false);
    }

    private async Task<OidcTokenResponse?> PostTokenRequestAsync(
        Uri tokenEndpoint,
        Dictionary<string, string> payload,
        CancellationToken cancellationToken)
    {
        using var content = new FormUrlEncodedContent(payload);
        using var request = new HttpRequestMessage(HttpMethod.Post, tokenEndpoint)
        {
            Content = content
        };

        try
        {
            var client = _httpClientFactory.CreateClient();
            using var response = await client.SendAsync(request, cancellationToken).ConfigureAwait(false);
            var body = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                _logger.LogWarning("OIDC token request failed: {Status} {Body}", response.StatusCode, body);
                return null;
            }

            var parsed = JsonSerializer.Deserialize<OidcTokenResponse>(body, JsonOptions);
            if (parsed is null || string.IsNullOrWhiteSpace(parsed.AccessToken))
            {
                _logger.LogWarning("OIDC token response missing access token.");
                return null;
            }

            return parsed;
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "OIDC token request failed.");
            return null;
        }
    }

    private static string NormalizeReturnUrl(string? returnUrl, CroniqOidcLoginOptions loginOptions)
    {
        var candidate = (returnUrl ?? string.Empty).Trim();
        if (string.IsNullOrWhiteSpace(candidate))
        {
            return "/";
        }

        if (Uri.TryCreate(candidate, UriKind.Absolute, out var absolute))
        {
            var baseUri = new Uri(loginOptions.UiBaseUrl);
            if (absolute.Scheme == baseUri.Scheme && absolute.Host == baseUri.Host && absolute.Port == baseUri.Port)
            {
                var normalized = absolute.PathAndQuery + absolute.Fragment;
                return string.IsNullOrWhiteSpace(normalized) ? "/" : normalized;
            }

            return "/";
        }

        if (!candidate.StartsWith('/') || candidate.StartsWith("//", StringComparison.Ordinal) || candidate.Contains('\\'))
        {
            return "/";
        }

        return candidate;
    }

    private static CookieOptions CreateStateCookieOptions(CroniqOidcLoginOptions loginOptions, DateTimeOffset issuedAt)
    {
        var ttlMinutes = loginOptions.StateTtlMinutes <= 0 ? 5 : loginOptions.StateTtlMinutes;
        return new CookieOptions
        {
            HttpOnly = true,
            Secure = ResolveSecure(loginOptions),
            SameSite = ResolveSameSite(loginOptions),
            Path = "/auth/oidc",
            Domain = loginOptions.CookieDomain,
            Expires = issuedAt.AddMinutes(ttlMinutes)
        };
    }

    private static void ClearStateCookie(HttpContext context, CroniqOidcLoginOptions loginOptions)
    {
        context.Response.Cookies.Delete(StateCookieName, new CookieOptions
        {
            HttpOnly = true,
            Secure = ResolveSecure(loginOptions),
            SameSite = ResolveSameSite(loginOptions),
            Path = "/auth/oidc",
            Domain = loginOptions.CookieDomain
        });
    }

    private static CookieOptions CreateRefreshCookieOptions(CroniqOidcLoginOptions loginOptions)
    {
        var expires = ResolveRefreshExpiry(loginOptions);
        return new CookieOptions
        {
            HttpOnly = true,
            Secure = ResolveSecure(loginOptions),
            SameSite = ResolveSameSite(loginOptions),
            Path = "/auth",
            Domain = loginOptions.CookieDomain,
            Expires = expires
        };
    }

    private static CookieOptions CreateCsrfCookieOptions(CroniqOidcLoginOptions loginOptions)
    {
        var expires = ResolveRefreshExpiry(loginOptions);
        return new CookieOptions
        {
            HttpOnly = false,
            Secure = ResolveSecure(loginOptions),
            SameSite = ResolveSameSite(loginOptions),
            Path = "/",
            Domain = loginOptions.CookieDomain,
            Expires = expires
        };
    }

    private static DateTimeOffset? ResolveRefreshExpiry(CroniqOidcLoginOptions loginOptions)
    {
        if (loginOptions.RefreshCookieLifetimeDays is > 0)
        {
            return DateTimeOffset.UtcNow.AddDays(loginOptions.RefreshCookieLifetimeDays.Value);
        }

        return null;
    }

    private static SameSiteMode ResolveSameSite(CroniqOidcLoginOptions loginOptions)
    {
        var raw = loginOptions.CookieSameSite?.Trim();
        if (string.IsNullOrWhiteSpace(raw))
        {
            return SameSiteMode.Lax;
        }

        return raw.Equals("None", StringComparison.OrdinalIgnoreCase) ? SameSiteMode.None
            : raw.Equals("Strict", StringComparison.OrdinalIgnoreCase) ? SameSiteMode.Strict
            : SameSiteMode.Lax;
    }

    private static bool ResolveSecure(CroniqOidcLoginOptions loginOptions)
    {
        if (ResolveSameSite(loginOptions) == SameSiteMode.None)
        {
            return true;
        }

        return loginOptions.CookieSecure;
    }

    private static string CreateCodeVerifier()
    {
        return CreateSecureToken(64);
    }

    public static string CreateCodeChallenge(string codeVerifier)
    {
        var bytes = SHA256.HashData(Encoding.ASCII.GetBytes(codeVerifier));
        return Base64UrlEncode(bytes);
    }

    private static string CreateSecureToken(int byteCount = 32)
    {
        var bytes = RandomNumberGenerator.GetBytes(byteCount);
        return Base64UrlEncode(bytes);
    }

    private static string Base64UrlEncode(byte[] bytes)
    {
        return Convert.ToBase64String(bytes)
            .TrimEnd('=')
            .Replace('+', '-')
            .Replace('/', '_');
    }

    private static Dictionary<string, object?>? TryReadJwtPayload(string token)
    {
        if (string.IsNullOrWhiteSpace(token))
        {
            return null;
        }

        var parts = token.Split('.');
        if (parts.Length != 3)
        {
            return null;
        }

        var payloadJson = TryBase64UrlDecodeToString(parts[1]);
        if (string.IsNullOrWhiteSpace(payloadJson))
        {
            return null;
        }

        try
        {
            return JsonSerializer.Deserialize<Dictionary<string, object?>>(payloadJson, JsonOptions);
        }
        catch
        {
            return null;
        }
    }

    private static string? TryBase64UrlDecodeToString(string value)
    {
        var normalized = value.Replace('-', '+').Replace('_', '/');
        var padLength = (4 - (normalized.Length % 4)) % 4;
        var padded = normalized + new string('=', padLength);
        try
        {
            var bytes = Convert.FromBase64String(padded);
            return Encoding.UTF8.GetString(bytes);
        }
        catch
        {
            return null;
        }
    }

    private static bool TryGetClaim(Dictionary<string, object?> payload, string? name, out string value)
    {
        value = string.Empty;
        if (string.IsNullOrWhiteSpace(name))
        {
            return false;
        }

        if (!payload.TryGetValue(name, out var raw) || raw is null)
        {
            return false;
        }

        if (raw is string str && !string.IsNullOrWhiteSpace(str))
        {
            value = str.Trim();
            return true;
        }

        if (raw is JsonElement element)
        {
            if (element.ValueKind == JsonValueKind.String)
            {
                var parsed = element.GetString();
                if (!string.IsNullOrWhiteSpace(parsed))
                {
                    value = parsed.Trim();
                    return true;
                }
            }

            if (element.ValueKind == JsonValueKind.Array)
            {
                foreach (var item in element.EnumerateArray())
                {
                    if (item.ValueKind == JsonValueKind.String)
                    {
                        var parsed = item.GetString();
                        if (!string.IsNullOrWhiteSpace(parsed))
                        {
                            value = parsed.Trim();
                            return true;
                        }
                    }
                }
            }
        }

        return false;
    }
}

internal sealed record OidcLoginState(
    string State,
    string CodeVerifier,
    string ReturnUrl,
    DateTimeOffset IssuedAtUtc);

internal sealed record OidcTokenResponse
{
    [JsonPropertyName("access_token")]
    public string? AccessToken { get; init; }

    [JsonPropertyName("refresh_token")]
    public string? RefreshToken { get; init; }

    [JsonPropertyName("token_type")]
    public string? TokenType { get; init; }

    [JsonPropertyName("expires_in")]
    public int? ExpiresIn { get; init; }
}
