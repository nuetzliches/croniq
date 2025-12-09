using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Provides changefeed events for webhook endpoint mutations.
/// </summary>
public interface IWebhookEndpointChangefeed
{
    Task<IReadOnlyCollection<WebhookEndpointEvent>> FetchAsync(long afterEventId, int maxBatchSize, CancellationToken cancellationToken);
}
