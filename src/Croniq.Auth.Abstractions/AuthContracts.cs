using System;
using System.Collections.Generic;

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
    string Name,
    bool IsActive,
    DateTimeOffset CreatedAt);

public sealed record TenantCreateRequest(
    string Name,
    string? TenantId = null);

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

public sealed record ApiClientUpsertRequest(
    string TenantId,
    string ClientId,
    string? Name,
    string? EnvironmentTag,
    IReadOnlyCollection<string>? Scopes,
    bool IsActive = true);

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
    string? EnvironmentTag,
    DateTimeOffset? ExpiresAt);

public sealed record ApiKeyValidationResult(
    bool IsValid,
    string? TenantId,
    string? EnvironmentTag,
    string? CallerId,
    IReadOnlyCollection<string> Scopes,
    string? Failure);

public sealed record CroniqTokenIssueRequest(
    string TenantId,
    string ClientId,
    string? EnvironmentTag,
    IReadOnlyCollection<string> Scopes,
    string? Audience,
    TimeSpan? Lifetime,
    IReadOnlyDictionary<string, object?>? AdditionalClaims = null);

public sealed record CroniqTokenIssueResult(
    string AccessToken,
    string TokenType,
    int ExpiresInSeconds);

/// <summary>Tenant CRUD for admin flows.</summary>
public interface ITenantStore
{
    Task<TenantDescriptor> CreateAsync(TenantCreateRequest request, CancellationToken cancellationToken = default);
    Task<TenantDescriptor?> GetByIdAsync(string tenantId, CancellationToken cancellationToken = default);
    Task<IReadOnlyCollection<TenantDescriptor>> ListAsync(CancellationToken cancellationToken = default);
    Task<bool> DeactivateAsync(string tenantId, CancellationToken cancellationToken = default);
}

/// <summary>API key issuance/validation and client metadata.</summary>
public interface IApiKeyStore
{
    Task<ApiKeyIssueResult> IssueAsync(ApiKeyIssueRequest request, CancellationToken cancellationToken = default);
    Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default);
    Task<ApiKeyIssueResult?> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default);
    Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default);
    Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default);
    Task<ApiClientDescriptor> UpsertClientAsync(ApiClientUpsertRequest request, CancellationToken cancellationToken = default);
    Task<IReadOnlyCollection<ApiClientDescriptor>> ListClientsAsync(string tenantId, string? environmentTag, CancellationToken cancellationToken = default);
    Task<bool> DeleteClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default);
}

/// <summary>User linkage to tenants and roles (OIDC-first).</summary>
public interface IUserStore
{
    Task<UserDescriptor> UpsertAsync(UserDescriptor descriptor, CancellationToken cancellationToken = default);
    Task<UserDescriptor?> FindBySubjectAsync(string issuer, string subject, CancellationToken cancellationToken = default);
    Task<IReadOnlyCollection<UserDescriptor>> ListByTenantAsync(string tenantId, CancellationToken cancellationToken = default);
}

/// <summary>Issues Croniq-signed bearer tokens for internal automation flows.</summary>
public interface ICroniqTokenIssuer
{
    Task<CroniqTokenIssueResult> IssueAsync(CroniqTokenIssueRequest request, CancellationToken cancellationToken = default);
}

public sealed record PasswordUserRecord(
    string UserId,
    string TenantId,
    string Username,
    IReadOnlyCollection<string> Scopes,
    string PasswordHash,
    bool IsActive,
    int FailedLoginCount,
    DateTimeOffset? LockoutEndUtc,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc,
    bool PasswordChangeRequired = false);

public sealed record PasswordUserUpsertRequest(
    string TenantId,
    string Username,
    string PasswordHash,
    IReadOnlyCollection<string> Scopes,
    bool IsActive = true,
    bool PasswordChangeRequired = false);

public interface IPasswordUserStore
{
    Task<PasswordUserRecord?> FindByUsernameAsync(string tenantId, string username, CancellationToken cancellationToken = default);
    Task<PasswordUserRecord?> FindByIdAsync(string tenantId, string userId, CancellationToken cancellationToken = default);
    Task<PasswordUserRecord> UpsertAsync(PasswordUserUpsertRequest request, CancellationToken cancellationToken = default);
    Task RecordLoginFailureAsync(string tenantId, string userId, DateTimeOffset? lockoutEndUtc, CancellationToken cancellationToken = default);
    Task RecordLoginSuccessAsync(string tenantId, string userId, CancellationToken cancellationToken = default);
}

public sealed record RefreshTokenRecord(
    string TokenId,
    string TenantId,
    string UserId,
    string TokenHash,
    DateTimeOffset ExpiresAtUtc,
    DateTimeOffset? RevokedAtUtc,
    string? ReplacedByTokenId,
    DateTimeOffset CreatedAtUtc);

public sealed record RefreshTokenCreateRequest(
    string TenantId,
    string UserId,
    string TokenHash,
    DateTimeOffset ExpiresAtUtc);

public interface IRefreshTokenStore
{
    Task<RefreshTokenRecord> CreateAsync(RefreshTokenCreateRequest request, CancellationToken cancellationToken = default);
    Task<RefreshTokenRecord?> FindActiveByHashAsync(string tenantId, string tokenHash, CancellationToken cancellationToken = default);
    Task RevokeAsync(string tenantId, string tokenId, string? replacedByTokenId, CancellationToken cancellationToken = default);
    Task RevokeAllForUserAsync(string tenantId, string userId, CancellationToken cancellationToken = default);
}
