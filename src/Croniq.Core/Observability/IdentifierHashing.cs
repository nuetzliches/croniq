using System;
using System.Security.Cryptography;
using System.Text;
using Croniq.Options;

namespace Croniq.Core.Observability;

public static class IdentifierHashing
{
    private sealed record Settings(bool Enabled, byte[]? Key);
    private static Settings _settings = new(false, null);

    public static void Configure(CroniqObservabilityOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);

        if (!options.HashIdentifiers)
        {
            _settings = new Settings(false, null);
            return;
        }

        if (string.IsNullOrWhiteSpace(options.IdentifierHashKey))
        {
            throw new InvalidOperationException("Croniq:Observability:IdentifierHashKey must be set when identifier hashing is enabled.");
        }

        var key = options.IdentifierHashKey.Trim();
        if (key.Length == 0)
        {
            throw new InvalidOperationException("Croniq:Observability:IdentifierHashKey must be set when identifier hashing is enabled.");
        }

        _settings = new Settings(true, Encoding.UTF8.GetBytes(key));
    }

    public static string? HashTenantId(string? tenantId)
        => HashIdentifier(tenantId);

    public static string? HashCallerId(string? callerId)
        => HashIdentifier(callerId);

    private static string? HashIdentifier(string? value)
    {
        var settings = _settings;
        if (!settings.Enabled || string.IsNullOrWhiteSpace(value))
        {
            return value;
        }

        var input = value.Trim();
        if (input.Length == 0)
        {
            return value;
        }

        using var hmac = new HMACSHA256(settings.Key!);
        var hash = hmac.ComputeHash(Encoding.UTF8.GetBytes(input));
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
