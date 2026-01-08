using System;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Webhooks.Remote;

public sealed class RemoteWebhookDeadLetterStore : IWebhookDeadLetterStore
{
    private readonly RemoteWebhookClient _client;

    public RemoteWebhookDeadLetterStore(RemoteWebhookClient client)
    {
        _client = client ?? throw new ArgumentNullException(nameof(client));
    }

    public Task<long> CreateAsync(WebhookDeadLetterCreate request, CancellationToken cancellationToken)
    {
        throw new InvalidOperationException("Remote webhook dead letters cannot be created from this host.");
    }

    public Task<IReadOnlyCollection<WebhookDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        return _client.ListDeadLettersAsync(scope, cancellationToken);
    }

    public async Task<WebhookDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        var entries = await _client.ListDeadLettersAsync(scope, cancellationToken).ConfigureAwait(false);
        return entries.FirstOrDefault(entry => entry.Id == id);
    }

    public Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        return _client.ResolveDeadLetterAsync(id, scope, cancellationToken);
    }

    public Task RecordFailureAsync(long id, PartitionScope scope, WebhookDeadLetterFailure failure, CancellationToken cancellationToken)
    {
        if (failure is null) throw new ArgumentNullException(nameof(failure));
        return _client.RecordDeadLetterFailureAsync(id, scope, failure, cancellationToken);
    }
}
