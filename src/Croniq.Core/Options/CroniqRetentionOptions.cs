namespace Croniq.Options;

public sealed class CroniqRetentionOptions
{
    /// <summary>
    /// Enables Croniq maintenance retention jobs.
    /// </summary>
    public bool Enabled { get; set; } = false;

    /// <summary>
    /// Cron schedule expression for the retention job trigger.
    /// Uses Croniq's 7-field cron format.
    /// Default: daily at 03:00 UTC.
    /// </summary>
    public string ScheduleCron { get; set; } = "0 0 3 ? * * *";

    /// <summary>
    /// Optional trigger id for the retention schedule.
    /// If null/empty a stable default is used.
    /// </summary>
    public string? TriggerId { get; set; }

    /// <summary>
    /// Optional time zone id used for computing fire times.
    /// Defaults to UTC when omitted.
    /// </summary>
    public string? TimeZoneId { get; set; }

    /// <summary>
    /// Enables pruning of expired refresh tokens in auth.RefreshTokens.
    /// </summary>
    public bool RefreshTokensEnabled { get; set; } = true;

    /// <summary>
    /// Retain refresh tokens for N days after they expire.
    /// Deletion condition: ExpiresAtUtc + N days &lt; now.
    /// Set to 0 to delete immediately after expiry, or -1 to disable refresh token pruning.
    /// </summary>
    public int RefreshTokensRetentionDays { get; set; } = 14;

    /// <summary>
    /// Enables pruning of Croniq job dead letters in croniq.DeadLetters.
    /// </summary>
    public bool JobDeadLettersEnabled { get; set; } = false;

    /// <summary>
    /// Retain job dead letters for N days after their ExpiresAtUtc timestamp.
    /// Deletion condition: ExpiresAtUtc + N days &lt; now.
    /// Set to 0 to delete as soon as ExpiresAtUtc is in the past, or -1 to disable.
    /// </summary>
    public int JobDeadLettersExpiryOffsetDays { get; set; } = 0;

    /// <summary>
    /// Enables pruning of webhook dead letters in croniq.WebhookDeadLetters.
    /// Only entries with a non-null ExpiresAtUtc are affected.
    /// </summary>
    public bool WebhookDeadLettersEnabled { get; set; } = false;

    /// <summary>
    /// Retain webhook dead letters for N days after their ExpiresAtUtc timestamp.
    /// Deletion condition: ExpiresAtUtc + N days &lt; now.
    /// Set to 0 to delete as soon as ExpiresAtUtc is in the past, or -1 to disable.
    /// </summary>
    public int WebhookDeadLettersExpiryOffsetDays { get; set; } = 0;

    /// <summary>
    /// Enables pruning of webhook endpoint events in croniq.WebhookEndpointEvents.
    /// Uses OccurredAtUtc as the retention baseline.
    /// </summary>
    public bool WebhookEndpointEventsEnabled { get; set; } = false;

    /// <summary>
    /// Retain webhook endpoint events for N days after OccurredAtUtc.
    /// Deletion condition: OccurredAtUtc + N days &lt; now.
    /// Set to -1 to disable.
    /// </summary>
    public int WebhookEndpointEventsRetentionDays { get; set; } = 30;

    /// <summary>
    /// Enables pruning of webhook secret history entries in croniq.WebhookSecretHistory.
    /// Only entries with a non-null ExpiresAtUtc are affected.
    /// </summary>
    public bool WebhookSecretHistoryEnabled { get; set; } = false;

    /// <summary>
    /// Retain webhook secret history entries for N days after ExpiresAtUtc.
    /// Deletion condition: ExpiresAtUtc + N days &lt; now.
    /// Set to 0 to delete immediately after expiry, or -1 to disable.
    /// </summary>
    public int WebhookSecretHistoryExpiryOffsetDays { get; set; } = 7;
}
