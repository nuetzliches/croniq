using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using Croniq.Auth.Abstractions;

namespace Croniq.Auth.Core;

public sealed record TenantSeed(
    string TenantId,
    string Name,
    bool IsActive = true,
    DateTimeOffset? CreatedAtUtc = null,
    string? Reference = null);

public sealed class InMemoryTenantStore : ITenantStore
{
    private readonly ConcurrentDictionary<string, TenantRecord> _tenantsById = new(StringComparer.OrdinalIgnoreCase);
    private readonly object _sync = new();

    public InMemoryTenantStore(IEnumerable<TenantSeed>? seeds = null)
    {
        if (seeds is null)
        {
            return;
        }

        foreach (var seed in seeds)
        {
            if (string.IsNullOrWhiteSpace(seed.TenantId) || string.IsNullOrWhiteSpace(seed.Name))
            {
                continue;
            }

            var descriptor = new TenantRecord(
                seed.TenantId.Trim(),
                seed.Name.Trim(),
                seed.Reference?.Trim() ?? seed.TenantId.Trim(),
                seed.IsActive,
                seed.CreatedAtUtc ?? DateTimeOffset.UtcNow);

            _tenantsById[descriptor.TenantId] = descriptor;
        }
    }

    public Task<TenantDescriptor> CreateAsync(TenantCreateRequest request, CancellationToken cancellationToken = default)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.Name)) throw new ArgumentException("Name is required", nameof(request));
        if (string.IsNullOrWhiteSpace(request.TenantId)) throw new ArgumentException("TenantId is required", nameof(request));

        var trimmedName = request.Name.Trim();
        TenantRecord record;

        var tenantId = request.TenantId.Trim();
        var reference = string.IsNullOrWhiteSpace(request.Reference)
            ? tenantId
            : request.Reference.Trim();

        lock (_sync)
        {
            if (_tenantsById.TryGetValue(tenantId, out var existing))
            {
                record = existing with { Name = trimmedName, Reference = reference, IsActive = true };
                _tenantsById[tenantId] = record;
            }
            else
            {
                record = new TenantRecord(tenantId, trimmedName, reference, true, DateTimeOffset.UtcNow);
                _tenantsById[tenantId] = record;
            }
        }

        return Task.FromResult(ToDescriptor(record));
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
            .OrderBy(d => d.TenantId, StringComparer.OrdinalIgnoreCase)
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
            record.Name,
            record.IsActive,
            record.CreatedAtUtc,
            record.Reference);
    }

    private sealed record TenantRecord(
        string TenantId,
        string Name,
        string Reference,
        bool IsActive,
        DateTimeOffset CreatedAtUtc);
}
