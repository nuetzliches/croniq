using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Xtraq.Core;

namespace Croniq.Persistence.Xtraq.Health;

public sealed class XtraqPersistenceHealth : IPersistenceHealth
{
    private readonly XtraqDbContext _dbContext;

    public XtraqPersistenceHealth(XtraqDbContext dbContext)
    {
        _dbContext = dbContext ?? throw new ArgumentNullException(nameof(dbContext));
    }

    public async Task<PersistenceHealthResult> CheckAsync(CancellationToken cancellationToken = default)
    {
        try
        {
            var result = await _dbContext.HealthPingAsync(cancellationToken).ConfigureAwait(false);
            return result.Success
                ? new PersistenceHealthResult(true)
                : new PersistenceHealthResult(false, result.Error ?? "Health probe failed.");
        }
        catch (Exception ex)
        {
            return new PersistenceHealthResult(false, ex.Message);
        }
    }
}
