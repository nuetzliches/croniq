namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Allows webhook persistence providers to signal that cached endpoint material should be refreshed.
/// </summary>
public interface IWebhookEndpointChangeNotifier
{
    /// <summary>
    /// Signals that the webhook endpoint identified by <paramref name="hookKey"/> has changed.
    /// </summary>
    /// <param name="hookKey">The unique hook key that changed.</param>
    void NotifyChanged(string hookKey);
}
