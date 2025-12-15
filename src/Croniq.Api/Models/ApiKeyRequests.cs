using System.Collections.Generic;
using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;
using Croniq.Auth.Abstractions;

namespace Croniq.Api.Models;

public sealed record IssueApiKeyRequest(
    [property: Required] string ClientId,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    int? TtlHours);

public sealed record IssueApiKeyResponse(
    string ClientId,
    string TenantId,
    string KeyId,
    string PlaintextSecret,
    DateTimeOffset? ExpiresAtUtc,
    string? EnvironmentTag);

public sealed record ApiClientResponse(
    string ClientId,
    string TenantId,
    string? Name,
    string? EnvironmentTag,
    IReadOnlyCollection<string> Scopes,
    bool IsActive,
    DateTimeOffset? ExpiresAtUtc);

public sealed record UpsertApiClientRequest(
    [property: Required] string ClientId,
    string? Name,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    bool? IsActive);

public sealed record IssueTokenRequest(
    string? ClientId,
    IReadOnlyCollection<string>? Scopes,
    string? Audience,
    int? TtlMinutes);

public sealed record IssueTokenResponse(
    string AccessToken,
    string TokenType,
    int ExpiresIn);

public sealed record CallerInfoResponse(
    string TenantId,
    string? EnvironmentTag,
    string CallerId,
    CallerType CallerType,
    IReadOnlyCollection<string> Scopes,
    bool IsActive);
