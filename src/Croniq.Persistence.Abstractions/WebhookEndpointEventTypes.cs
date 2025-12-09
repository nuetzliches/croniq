namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Known changefeed events for webhook endpoints.
/// </summary>
public static class WebhookEndpointEventTypes
{
    public const string Created = "created";
    public const string Updated = "updated";
    public const string Deleted = "deleted";
}
