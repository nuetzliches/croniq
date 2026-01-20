using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Webhooks.Remote;

public sealed class RemoteWebhookActivityStore : IWebhookActivityStore
{
    private readonly RemoteWebhookClient _client;

    public RemoteWebhookActivityStore(RemoteWebhookClient client)
    {
        _client = client ?? throw new ArgumentNullException(nameof(client));
    }

    public Task<IReadOnlyCollection<WebhookActivityEntry>> ListAsync(
        PartitionScope scope,
        WebhookActivityQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));
        return _client.ListActivityAsync(scope, query, cancellationToken);
    }

    public Task<WebhookActivitySummary> SummarizeAsync(
        PartitionScope scope,
        WebhookActivitySummaryQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));
        return _client.SummarizeActivityAsync(scope, query, cancellationToken);
    }
}
