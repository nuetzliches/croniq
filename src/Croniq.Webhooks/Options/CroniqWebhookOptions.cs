namespace Croniq.Webhooks.Options;

public sealed class CroniqWebhookOptions
{
    public WebhookPersistenceMode Mode { get; set; } = WebhookPersistenceMode.InMemory;

    public int RequestsPerMinute { get; set; } = 60;

    public IList<WebhookEndpointOptions> Endpoints { get; } = new List<WebhookEndpointOptions>();

    public WebhookDeadLetterOptions DeadLetter { get; set; } = new();

    public WebhookCacheOptions Cache { get; set; } = new();

    public WebhookSecurityOptions Security { get; set; } = new();

    public WebhookSqlServerOptions SqlServer { get; set; } = new();

    public WebhookRemoteOptions Remote { get; set; } = new();

    public WebhookIngressOptions Ingress { get; set; } = new();

    public bool ConfigurePersistence { get; set; } = true;
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

public sealed class WebhookCacheOptions
{
    public bool ChangefeedEnabled { get; set; } = true;

    public int PollingIntervalSeconds { get; set; } = 3;

    public int BatchSize { get; set; } = 128;
}

public sealed class WebhookSecurityOptions
{
    public bool AllowUnsignedHooks { get; set; } = false;
}

public sealed class WebhookIngressOptions
{
    public WebhookIngressDispatchMode DispatchMode { get; set; } = WebhookIngressDispatchMode.TriggerJob;
}

public enum WebhookPersistenceMode
{
    InMemory,
    SqlServer,
    Remote
}

public enum WebhookIngressDispatchMode
{
    TriggerJob,
    StoreOnly
}

public enum WebhookIngressStreamMode
{
    Grpc,
    Sse,
    Polling
}

public sealed class WebhookSqlServerOptions
{
    public string? ConnectionString { get; set; }

    public string? MigrationsAssembly { get; set; }

    public bool? EnableDetailedErrors { get; set; }

    public bool? EnableSensitiveDataLogging { get; set; }
}

public sealed class WebhookRemoteOptions
{
    public string? BaseUrl { get; set; }

    public string? ApiKey { get; set; }

    public int TimeoutSeconds { get; set; } = 10;

    public WebhookIngressStreamMode StreamMode { get; set; } = WebhookIngressStreamMode.Grpc;

    public WebhookIngressStreamMode? StreamFallback { get; set; }

    public int MaxInflight { get; set; } = 100;

    public int ReconnectDelaySeconds { get; set; } = 5;

    public bool EnableRelay { get; set; } = true;
}
