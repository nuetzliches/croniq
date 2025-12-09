namespace Croniq.Webhooks.Options;

public sealed class CroniqWebhookOptions
{
    public int RequestsPerMinute { get; set; } = 60;

    public IList<WebhookEndpointOptions> Endpoints { get; } = new List<WebhookEndpointOptions>();

    public WebhookDeadLetterOptions DeadLetter { get; set; } = new();
}

public sealed class WebhookEndpointOptions
{
    public string HookKey { get; set; } = string.Empty;

    public string JobKey { get; set; } = string.Empty;

    public string? Secret { get; set; }

    public bool RequireSignature { get; set; } = true;

    public int? RequestsPerMinute { get; set; }

    public IDictionary<string, string>? Metadata { get; set; }

    public bool Enabled { get; set; } = true;
}

public sealed class WebhookDeadLetterOptions
{
    public bool Enabled { get; set; } = true;

    public int RetentionDays { get; set; } = 14;
}
