using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;

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
