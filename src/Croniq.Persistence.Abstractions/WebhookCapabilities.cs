using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public sealed record WebhookCapabilities(
    bool AllowUnsignedHooks,
    int DefaultRequestsPerMinute);

public interface IWebhookCapabilitiesProvider
{
    Task<WebhookCapabilities> GetCapabilitiesAsync(PartitionScope scope, CancellationToken cancellationToken);
}
