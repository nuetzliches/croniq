namespace Croniq.Api;

public sealed class WebhookIngressStreamOptions
{
    public int LeaseSeconds { get; set; } = 30;

    public int MaxBatchSize { get; set; } = 100;

    public int PollingIntervalMilliseconds { get; set; } = 250;
}
