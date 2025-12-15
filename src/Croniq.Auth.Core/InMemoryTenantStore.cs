using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using Croniq.Auth.Abstractions;

namespace Croniq.Auth.Core;

public sealed record TenantSeed(
    string TenantId,
    string Reference,
    string Name,
    bool IsActive = true,
    DateTimeOffset? CreatedAtUtc = null);

public sealed class InMemoryTenantStore : ITenantStore
{
    private readonly ConcurrentDictionary<string, TenantRecord> _tenantsById = new(StringComparer.OrdinalIgnoreCase);
    private readonly ConcurrentDictionary<string, string> _referenceIndex = new(StringComparer.OrdinalIgnoreCase);
    private readonly object _sync = new();

    public InMemoryTenantStore(IEnumerable<TenantSeed>? seeds = null)
    {
        if (seeds is null)
        {
            return;
        }

        foreach (var seed in seeds)
        {
            if (string.IsNullOrWhiteSpace(seed.TenantId) || string.IsNullOrWhiteSpace(seed.Reference) || string.IsNullOrWhiteSpace(seed.Name))
            {
                continue;
            }

            var descriptor = new TenantRecord(
                seed.TenantId.Trim(),
                seed.Reference.Trim(),
                seed.Name.Trim(),
                seed.IsActive,
                seed.CreatedAtUtc ?? DateTimeOffset.UtcNow);

            _tenantsById[descriptor.TenantId] = descriptor;
            _referenceIndex[NormalizeReference(descriptor.Reference)] = descriptor.TenantId;
        }
    }

    public Task<TenantDescriptor> CreateAsync(string reference, string name, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(reference)) throw new ArgumentException("Reference is required", nameof(reference));
        if (string.IsNullOrWhiteSpace(name)) throw new ArgumentException("Name is required", nameof(name));

        var normalizedReference = NormalizeReference(reference);
        var trimmedName = name.Trim();
        TenantRecord record;

        lock (_sync)
        {
            if (_referenceIndex.TryGetValue(normalizedReference, out var tenantId)
                && _tenantsById.TryGetValue(tenantId, out var existing))
            {
                record = existing with { Name = trimmedName, IsActive = true };
                _tenantsById[tenantId] = record;
            }
            else
            {
                var newTenantId = GenerateTenantId();
                record = new TenantRecord(newTenantId, reference.Trim(), trimmedName, true, DateTimeOffset.UtcNow);
                _tenantsById[newTenantId] = record;
                _referenceIndex[normalizedReference] = newTenantId;
            }
        }

        return Task.FromResult(ToDescriptor(record));
    }

    public Task<TenantDescriptor?> GetByReferenceAsync(string reference, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(reference)) throw new ArgumentException("Reference is required", nameof(reference));

        var normalized = NormalizeReference(reference);
        if (_referenceIndex.TryGetValue(normalized, out var tenantId)
            && _tenantsById.TryGetValue(tenantId, out var record))
        {
            return Task.FromResult<TenantDescriptor?>(ToDescriptor(record));
        }

        return Task.FromResult<TenantDescriptor?>(null);
    }

    public Task<TenantDescriptor?> GetByIdAsync(string tenantId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        if (_tenantsById.TryGetValue(tenantId, out var record))
        {
            return Task.FromResult<TenantDescriptor?>(ToDescriptor(record));
        }

        return Task.FromResult<TenantDescriptor?>(null);
    }

    public Task<IReadOnlyCollection<TenantDescriptor>> ListAsync(CancellationToken cancellationToken = default)
    {
        var descriptors = _tenantsById.Values
            .Select(ToDescriptor)
            .OrderBy(d => d.Reference, StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return Task.FromResult<IReadOnlyCollection<TenantDescriptor>>(descriptors);
    }

    public Task<bool> DeactivateAsync(string tenantId, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(tenantId)) throw new ArgumentException("TenantId is required", nameof(tenantId));

        lock (_sync)
        {
            if (!_tenantsById.TryGetValue(tenantId, out var existing))
            {
                return Task.FromResult(false);
            }

            _tenantsById[tenantId] = existing with { IsActive = false };
            return Task.FromResult(true);
        }
    }

    private static TenantDescriptor ToDescriptor(TenantRecord record)
    {
        return new TenantDescriptor(
            record.TenantId,
            record.Reference,
            record.Name,
            record.IsActive,
            record.CreatedAtUtc);
    }

    private static string NormalizeReference(string reference) => reference.Trim().ToLowerInvariant();

    private static string GenerateTenantId() => $"tn_{Guid.NewGuid():N}";

    private sealed record TenantRecord(
        string TenantId,
        string Reference,
        string Name,
        bool IsActive,
        DateTimeOffset CreatedAtUtc);
}
