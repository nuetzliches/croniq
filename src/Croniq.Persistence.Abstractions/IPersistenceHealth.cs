using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Health probe for a persistence provider.
/// </summary>
public interface IPersistenceHealth
{
    Task<PersistenceHealthResult> CheckAsync(CancellationToken cancellationToken = default);
}

public sealed record PersistenceHealthResult(bool IsHealthy, string? Detail = null);
