using System.Globalization;
using System.Text.Json;
using Croniq.Auth.Abstractions;
using Croniq.Persistence.Xtraq;
using XtraqAuth = Croniq.Persistence.Xtraq.Auth;
using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Auth.Xtraq;

public sealed class XtraqApiKeyStore : IApiKeyStore
{
    private readonly IXtraqDbContext _dbContext;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public XtraqApiKeyStore(IXtraqDbContext dbContext)
    {
        _dbContext = dbContext ?? throw new ArgumentNullException(nameof(dbContext));
    }

    public async Task<ApiKeyIssueResult> IssueAsync(ApiKeyIssueRequest request, CancellationToken cancellationToken = default)
    {
        var ttlMinutes = request.Ttl.HasValue ? (int?)Math.Ceiling(request.Ttl.Value.TotalMinutes) : null;
        var dbResult = await XtraqAuth.ApiKeyIssueExtensions.ApiKeyIssueAsync(
            _dbContext,
            new XtraqAuth.ApiKeyIssueInput(
                TenantId: ParseId(request.TenantId),
                ClientId: ParseId(request.ClientId),
                Environment: request.EnvironmentTag,
                Scopes: SerializeScopes(request.Scopes),
                TtlMinutes: ttlMinutes,
                CreatedBy: "system"),
            cancellationToken).ConfigureAwait(false);

        var output = dbResult.Output ?? throw new InvalidOperationException("ApiKeyIssue did not produce an output payload.");
        var keyId = (output.KeyId ?? throw new InvalidOperationException("ApiKeyIssue did not return a key id.")).ToString(CultureInfo.InvariantCulture);
        var plaintextKey = output.PlaintextKey ?? throw new InvalidOperationException("ApiKeyIssue did not return plaintext key.");
        DateTimeOffset? expiresAt = null;
        if (output.ExpiresUtc.HasValue)
        {
            var utc = DateTime.SpecifyKind(output.ExpiresUtc.Value, DateTimeKind.Utc);
            expiresAt = new DateTimeOffset(utc);
        }

        return new ApiKeyIssueResult(
            request.ClientId,
            request.TenantId,
            keyId,
            plaintextKey,
            expiresAt);
    }

    public async Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        var result = await XtraqAuth.ApiKeyRevokeExtensions.ApiKeyRevokeAsync(
            _dbContext,
            new XtraqAuth.ApiKeyRevokeInput(
                TenantId: ParseId(tenantId),
                KeyRef: keyId,
                Actor: "system",
                Reason: null),
            cancellationToken).ConfigureAwait(false);

        var affected = result.Output?.Affected ?? 0;
        return affected > 0;
    }

    public async Task<bool> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        var result = await XtraqAuth.ApiKeyRotateExtensions.ApiKeyRotateAsync(
            _dbContext,
            new XtraqAuth.ApiKeyRotateInput(
                TenantId: ParseId(tenantId),
                KeyRef: keyId,
                Actor: "system"),
            cancellationToken).ConfigureAwait(false);

        return result.Output?.PlaintextKey is { Length: > 0 };
    }

    public async Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        var result = await XtraqAuth.ApiKeyValidateExtensions.ApiKeyValidateAsync(
            _dbContext,
            new XtraqAuth.ApiKeyValidateInput(presentedKey),
            cancellationToken).ConfigureAwait(false);

        if (result.Result.Count == 0)
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "not-found");
        }

        var first = result.Result[0];
        var tenantId = first.TenantId.ToString(CultureInfo.InvariantCulture);
        var scopes = ParseScopes(first.Scopes);

        return new ApiKeyValidationResult(first.IsValid, tenantId, first.Environment, first.CallerId, scopes, first.Failure);
    }

    public async Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        var result = await XtraqAuth.ApiClientGetExtensions.ApiClientGetAsync(
            _dbContext,
            new XtraqAuth.ApiClientGetInput(
                TenantId: ParseId(tenantId),
                ClientId: ParseId(clientId)),
            cancellationToken).ConfigureAwait(false);

        if (result.Result.Count == 0)
        {
            return null;
        }

        var row = result.Result[0];
        var scopes = ParseScopes(row.Scopes);

        return new ApiClientDescriptor(
            clientId,
            tenantId,
            row.Name,
            row.Environment,
            scopes,
            !row.IsDeleted,
            null);
    }

    private IReadOnlyCollection<string> ParseScopes(string? scopesJson)
    {
        if (string.IsNullOrWhiteSpace(scopesJson)) return Array.Empty<string>();
        return JsonSerializer.Deserialize<string[]>(scopesJson, _jsonOptions) ?? Array.Empty<string>();
    }

    private string? SerializeScopes(IReadOnlyCollection<string>? scopes)
    {
        return scopes is null || scopes.Count == 0 ? null : JsonSerializer.Serialize(scopes, _jsonOptions);
    }

    private static int ParseId(string value)
    {
        return int.Parse(value, CultureInfo.InvariantCulture);
    }
}

public static class CroniqAuthXtraqServiceCollectionExtensions
{
    public static IServiceCollection AddCroniqAuthXtraq(this IServiceCollection services)
    {
        services.AddScoped<IApiKeyStore, XtraqApiKeyStore>();
        return services;
    }
}
