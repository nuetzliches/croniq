using System.Data;
using System.Globalization;
using System.Text.Json;
using Croniq.Auth.Abstractions;
using Croniq.Persistence.Xtraq;
using Microsoft.Data.SqlClient;
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
        await using var connection = await _dbContext.OpenConnectionAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = connection.CreateCommand();
        cmd.CommandType = CommandType.StoredProcedure;
        cmd.CommandText = "[auth].[ApiKeyIssue]";
        cmd.Parameters.Add(new SqlParameter("@TenantId", SqlDbType.Int) { Value = int.Parse(request.TenantId) });
        cmd.Parameters.Add(new SqlParameter("@ClientId", SqlDbType.Int) { Value = int.Parse(request.ClientId) });
        var ttlMinutes = request.Ttl.HasValue ? (int?)Math.Ceiling(request.Ttl.Value.TotalMinutes) : null;
        cmd.Parameters.Add(new SqlParameter("@Environment", SqlDbType.NVarChar, 32) { Value = (object?)request.EnvironmentTag ?? DBNull.Value });
        cmd.Parameters.Add(new SqlParameter("@Scopes", SqlDbType.NVarChar, -1) { Value = (object?)SerializeScopes(request.Scopes) ?? DBNull.Value });
        cmd.Parameters.Add(new SqlParameter("@TtlMinutes", SqlDbType.Int) { Value = (object?)ttlMinutes ?? DBNull.Value });
        cmd.Parameters.Add(new SqlParameter("@CreatedBy", SqlDbType.NVarChar, 128) { Value = "system" });
        var keyIdParam = new SqlParameter("@KeyId", SqlDbType.Int) { Direction = ParameterDirection.Output };
        var keyRefParam = new SqlParameter("@KeyRef", SqlDbType.NVarChar, 64) { Direction = ParameterDirection.Output };
        var plaintextParam = new SqlParameter("@PlaintextKey", SqlDbType.NVarChar, 512) { Direction = ParameterDirection.Output };
        var previewParam = new SqlParameter("@SecretPreview", SqlDbType.NVarChar, 16) { Direction = ParameterDirection.Output };
        var expiresParam = new SqlParameter("@ExpiresAtUtc", SqlDbType.DateTime2) { Direction = ParameterDirection.Output, IsNullable = true };
        cmd.Parameters.Add(keyIdParam);
        cmd.Parameters.Add(keyRefParam);
        cmd.Parameters.Add(plaintextParam);
        cmd.Parameters.Add(previewParam);
        cmd.Parameters.Add(expiresParam);

        await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);

        var keyId = Convert.ToInt32(keyIdParam.Value, CultureInfo.InvariantCulture).ToString(CultureInfo.InvariantCulture);
        var plaintextKey = plaintextParam.Value as string ?? throw new InvalidOperationException("ApiKeyIssue did not return plaintext key.");
        var expiresAt = expiresParam.Value is DBNull ? (DateTimeOffset?)null : DateTime.SpecifyKind((DateTime)expiresParam.Value, DateTimeKind.Utc);

        return new ApiKeyIssueResult(
            request.ClientId,
            request.TenantId,
            keyId,
            plaintextKey,
            expiresAt);
    }

    public async Task<bool> RevokeAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        await using var connection = await _dbContext.OpenConnectionAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = connection.CreateCommand();
        cmd.CommandType = CommandType.StoredProcedure;
        cmd.CommandText = "[auth].[ApiKeyRevoke]";
        cmd.Parameters.Add(new SqlParameter("@TenantId", SqlDbType.Int) { Value = int.Parse(tenantId) });
        cmd.Parameters.Add(new SqlParameter("@KeyRef", SqlDbType.NVarChar, 64) { Value = keyId });
        cmd.Parameters.Add(new SqlParameter("@Actor", SqlDbType.NVarChar, 128) { Value = "system" });
        cmd.Parameters.Add(new SqlParameter("@Reason", SqlDbType.NVarChar, 64) { Value = DBNull.Value });
        var affectedParam = new SqlParameter("@Affected", SqlDbType.Int) { Direction = ParameterDirection.Output };
        cmd.Parameters.Add(affectedParam);

        await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
        return Convert.ToInt32(affectedParam.Value ?? 0, CultureInfo.InvariantCulture) > 0;
    }

    public async Task<bool> RotateAsync(string tenantId, string keyId, CancellationToken cancellationToken = default)
    {
        await using var connection = await _dbContext.OpenConnectionAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = connection.CreateCommand();
        cmd.CommandType = CommandType.StoredProcedure;
        cmd.CommandText = "[auth].[ApiKeyRotate]";
        cmd.Parameters.Add(new SqlParameter("@TenantId", SqlDbType.Int) { Value = int.Parse(tenantId) });
        cmd.Parameters.Add(new SqlParameter("@KeyRef", SqlDbType.NVarChar, 64) { Value = keyId });
        cmd.Parameters.Add(new SqlParameter("@Actor", SqlDbType.NVarChar, 128) { Value = "system" });
        var plaintextParam = new SqlParameter("@PlaintextKey", SqlDbType.NVarChar, 512) { Direction = ParameterDirection.Output };
        var previewParam = new SqlParameter("@SecretPreview", SqlDbType.NVarChar, 16) { Direction = ParameterDirection.Output };
        cmd.Parameters.Add(plaintextParam);
        cmd.Parameters.Add(previewParam);

        await cmd.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
        return plaintextParam.Value is not null && plaintextParam.Value != DBNull.Value;
    }

    public async Task<ApiKeyValidationResult> ValidateAsync(string presentedKey, CancellationToken cancellationToken = default)
    {
        await using var connection = await _dbContext.OpenConnectionAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = connection.CreateCommand();
        cmd.CommandType = CommandType.StoredProcedure;
        cmd.CommandText = "[auth].[ApiKeyValidate]";
        cmd.Parameters.Add(new SqlParameter("@Presented", SqlDbType.NVarChar, 512) { Value = presentedKey });

        await using var reader = await cmd.ExecuteReaderAsync(cancellationToken).ConfigureAwait(false);
        if (!await reader.ReadAsync(cancellationToken).ConfigureAwait(false))
        {
            return new ApiKeyValidationResult(false, null, null, null, Array.Empty<string>(), "not-found");
        }

        var isValid = reader.GetBoolean(reader.GetOrdinal("IsValid"));
        var tenantId = reader.IsDBNull(reader.GetOrdinal("TenantId")) ? null : reader.GetInt32(reader.GetOrdinal("TenantId")).ToString(CultureInfo.InvariantCulture);
        var env = reader.IsDBNull(reader.GetOrdinal("Environment")) ? null : reader.GetString(reader.GetOrdinal("Environment"));
        var callerId = reader.IsDBNull(reader.GetOrdinal("CallerId")) ? null : reader.GetString(reader.GetOrdinal("CallerId"));
        var scopes = ParseScopes(reader.IsDBNull(reader.GetOrdinal("Scopes")) ? null : reader.GetString(reader.GetOrdinal("Scopes")));
        var failure = reader.IsDBNull(reader.GetOrdinal("Failure")) ? null : reader.GetString(reader.GetOrdinal("Failure"));

        return new ApiKeyValidationResult(isValid, tenantId, env, callerId, scopes, failure);
    }

    public async Task<ApiClientDescriptor?> GetClientAsync(string tenantId, string clientId, CancellationToken cancellationToken = default)
    {
        await using var connection = await _dbContext.OpenConnectionAsync(cancellationToken).ConfigureAwait(false);
        await using var cmd = connection.CreateCommand();
        cmd.CommandType = CommandType.StoredProcedure;
        cmd.CommandText = "[auth].[ApiClientGet]";
        cmd.Parameters.Add(new SqlParameter("@TenantId", SqlDbType.Int) { Value = int.Parse(tenantId) });
        cmd.Parameters.Add(new SqlParameter("@ClientId", SqlDbType.Int) { Value = int.Parse(clientId) });

        await using var reader = await cmd.ExecuteReaderAsync(cancellationToken).ConfigureAwait(false);
        if (!await reader.ReadAsync(cancellationToken).ConfigureAwait(false))
        {
            return null;
        }

        var env = reader.IsDBNull(reader.GetOrdinal("Environment")) ? null : reader.GetString(reader.GetOrdinal("Environment"));
        var scopes = ParseScopes(reader.IsDBNull(reader.GetOrdinal("Scopes")) ? null : reader.GetString(reader.GetOrdinal("Scopes")));
        var isDeleted = reader.IsDBNull(reader.GetOrdinal("IsDeleted")) ? false : reader.GetBoolean(reader.GetOrdinal("IsDeleted"));
        return new ApiClientDescriptor(
            clientId,
            tenantId,
            reader.IsDBNull(reader.GetOrdinal("Name")) ? null : reader.GetString(reader.GetOrdinal("Name")),
            env,
            scopes,
            !isDeleted,
            null);
    }

    private IReadOnlyCollection<string> ParseScopes(string? scopesJson)
    {
        if (string.IsNullOrWhiteSpace(scopesJson)) return Array.Empty<string>();
        return JsonSerializer.Deserialize<string[]>(scopesJson, _jsonOptions) ?? Array.Empty<string>();
    }

    private static string? SerializeScopes(IReadOnlyCollection<string>? scopes)
    {
        return scopes is null || scopes.Count == 0 ? null : JsonSerializer.Serialize(scopes);
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
