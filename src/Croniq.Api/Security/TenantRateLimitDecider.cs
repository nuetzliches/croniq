using Croniq.Auth.Abstractions;
using Microsoft.Extensions.Options;
using System.Security.Cryptography;
using System.Text;

namespace Croniq.Api.Security;

internal sealed class TenantRateLimitDecider
{
    private readonly IOptionsMonitor<CroniqApiOptions> _apiOptions;

    public TenantRateLimitDecider(IOptionsMonitor<CroniqApiOptions> apiOptions)
    {
        _apiOptions = apiOptions ?? throw new ArgumentNullException(nameof(apiOptions));
    }

    public string GetPartitionId(ICallerContext? caller, string? fallback)
    {
        if (caller is not null && !string.IsNullOrWhiteSpace(caller.TenantId))
        {
            var tenant = caller.TenantId.Trim();
            var callerId = string.IsNullOrWhiteSpace(caller.CallerId) ? "caller" : caller.CallerId.Trim();
            return $"tenant:{tenant}|caller:{callerId}";
        }

        if (!string.IsNullOrWhiteSpace(fallback))
        {
            return $"anonymous:{HashPartitionKey(fallback)}";
        }

        return "anonymous:";
    }

    public int GetPermitLimit(ICallerContext? caller)
    {
        return GetPermitLimit(caller?.TenantId);
    }

    public int GetPermitLimit(string? tenantId)
    {
        var options = _apiOptions.CurrentValue ?? new CroniqApiOptions();
        if (!string.IsNullOrWhiteSpace(tenantId)
            && options.TenantRateLimits.TryGetValue(tenantId, out var tenant)
            && tenant is not null
            && tenant.RequestsPerMinute > 0)
        {
            return tenant.RequestsPerMinute;
        }

        return Math.Max(1, options.RequestsPerMinute);
    }

    private static string HashPartitionKey(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
