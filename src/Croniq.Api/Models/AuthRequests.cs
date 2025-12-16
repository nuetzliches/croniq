namespace Croniq.Api.Models;

public sealed record PasswordLoginRequest(
    string? TenantId,
    string Username,
    string Password,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    string? Audience,
    string? TenantReference = null);

public sealed record PasswordRefreshRequest(
    string? TenantId,
    string RefreshToken,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    string? Audience);

public sealed record PasswordLogoutRequest(
    string? TenantId,
    string RefreshToken);
