namespace Croniq.Auth.Abstractions;

/// <summary>Caller identity for the current request (API key or user token).</summary>
public interface ICallerContext
{
    string TenantId { get; }
    string? EnvironmentTag { get; }
    CallerType CallerType { get; }
    string CallerId { get; }
    IReadOnlyCollection<string> Scopes { get; }
    bool IsActive { get; }
}

/// <summary>Provides access to the ambient caller context (per request scope).</summary>
public interface ICallerContextAccessor
{
    ICallerContext? Current { get; set; }
}

/// <summary>Creates a caller context from presented credentials (API key or bearer token).</summary>
public interface ICallerContextFactory
{
    Task<ICallerContext?> FromApiKeyAsync(string presentedKey, CancellationToken cancellationToken = default);
    Task<ICallerContext?> FromBearerTokenAsync(string bearerToken, CancellationToken cancellationToken = default);
}

public enum CallerType
{
    ApiKey,
    User
}

public sealed record TenantDescriptor(
    string TenantId,
    string Reference,
    string Name,
    bool IsActive,
    DateTimeOffset CreatedAt);

public sealed record UserDescriptor(
    string UserId,
    string TenantId,
    string Subject,
    string Issuer,
    string? Email,
    string? DisplayName,
    IReadOnlyCollection<string> Roles,
    bool IsActive);

public sealed record ApiClientDescriptor(
    string ClientId,
    string TenantId,
    string? Name,
    string? EnvironmentTag,
    IReadOnlyCollection<string> Scopes,
    bool IsActive,
    DateTimeOffset? ExpiresAt);

public sealed record ApiKeyIssueRequest(
    string TenantId,
    string ClientId,
    string? EnvironmentTag,
    IReadOnlyCollection<string> Scopes,
    TimeSpan? Ttl);

public sealed record ApiKeyIssueResult(
    string ClientId,
    string TenantId,
    string KeyId,
    string PlaintextSecret,
    DateTimeOffset? ExpiresAt);

public sealed record ApiKeyValidationResult(
    bool IsValid,
    string? TenantId,
    string? EnvironmentTag,
    string? CallerId,
    IReadOnlyCollection<string> Scopes,
    string? Failure);

/// <summary>Tenant CRUD for admin flows.</summary>
public interface ITenantStore
{
    Task<TenantDescriptor> CreateAsync(string reference, string name, CancellationToken cancellationToken = default);
    Task<TenantDescriptor?> GetByReferenceAsync(string reference, CancellationToken cancellationToken = default);
    Task<TenantDescriptor?> GetByIdAsync(string tenantId, CancellationToken cancellationToken = default);
    Task<IReadOnlyCollection<TenantDescriptor>> ListAsync(CancellationToken cancellationToken = default);
    Task<bool> DeactivateAsync(string tenantId, CancellationToken cancellationToken = default);
}

/// <summary>API key issuance/validation and client metadata.</summary>
public interface IApiKeyStore
{
    Task<ApiKeyIssueResult> IssueAsync(ApiKeyIssueRequest request, CancellationToken cancellationToken = default);
    Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default);
    Task<bool> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default);
    Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default);
    Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default);
}

/// <summary>User linkage to tenants and roles (OIDC-first).</summary>
public interface IUserStore
{
    Task<UserDescriptor> UpsertAsync(UserDescriptor descriptor, CancellationToken cancellationToken = default);
    Task<UserDescriptor?> FindBySubjectAsync(string issuer, string subject, CancellationToken cancellationToken = default);
    Task<IReadOnlyCollection<UserDescriptor>> ListByTenantAsync(string tenantId, CancellationToken cancellationToken = default);
}
