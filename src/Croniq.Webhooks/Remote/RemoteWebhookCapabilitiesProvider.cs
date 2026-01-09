using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Webhooks.Remote;

public sealed class RemoteWebhookCapabilitiesProvider : IWebhookCapabilitiesProvider
{
    private readonly RemoteWebhookClient _client;

    public RemoteWebhookCapabilitiesProvider(RemoteWebhookClient client)
    {
        _client = client ?? throw new ArgumentNullException(nameof(client));
    }

    public Task<WebhookCapabilities> GetCapabilitiesAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        return _client.GetCapabilitiesAsync(scope, cancellationToken);
    }
}
