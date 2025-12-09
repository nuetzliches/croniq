using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public interface IWebhookDeadLetterStore
{
    Task<long> CreateAsync(WebhookDeadLetterCreate request, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<WebhookDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken);

    Task<WebhookDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken);

    Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken);

    Task RecordFailureAsync(long id, PartitionScope scope, WebhookDeadLetterFailure failure, CancellationToken cancellationToken);
}
