namespace Croniq.Api.Models;

public sealed record PasswordLoginRequest(
    string Username,
    string Password,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    string? Audience,
    string? TenantReference = null);

public sealed record PasswordRefreshRequest(
    string RefreshToken,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    string? Audience,
    string? TenantReference = null);

public sealed record PasswordLogoutRequest(
    string RefreshToken,
    string? TenantReference = null);

public sealed record PasswordChangePasswordRequest(
    string CurrentPassword,
    string NewPassword);
