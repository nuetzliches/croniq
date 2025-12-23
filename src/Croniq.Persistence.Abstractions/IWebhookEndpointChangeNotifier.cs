namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Allows webhook persistence providers to signal that cached endpoint material should be refreshed.
/// </summary>
public interface IWebhookEndpointChangeNotifier
{
    /// <summary>
    /// Signals that the webhook endpoint identified by <paramref name="hookKey"/> has changed within the specified scope.
    /// </summary>
    /// <param name="hookKey">The hook key that changed.</param>
    /// <param name="scope">Tenant/environment scope of the webhook endpoint.</param>
    void NotifyChanged(string hookKey, PartitionScope scope);
}
