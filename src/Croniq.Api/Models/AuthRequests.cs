namespace Croniq.Api.Models;

public sealed record PasswordLoginRequest(
    string Username,
    string Password,
    string TenantId,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    string? Audience);

public sealed record PasswordRefreshRequest(
    string RefreshToken,
    string TenantId,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    string? Audience);

public sealed record PasswordLogoutRequest(
    string RefreshToken,
    string TenantId);

public sealed record PasswordChangePasswordRequest(
    string CurrentPassword,
    string NewPassword);

public sealed record PasswordAuthResponse(
    string TenantId,
    string AccessToken,
    string TokenType,
    int? ExpiresIn,
    string RefreshToken,
    bool PasswordChangeRequired);

public sealed record OidcAuthResponse(
    string AccessToken,
    string TokenType,
    int? ExpiresIn,
    string? TenantId);
