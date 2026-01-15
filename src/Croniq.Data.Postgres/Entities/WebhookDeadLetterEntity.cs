using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

/// <summary>
/// Captures failed webhook ingress attempts for replay/inspection.
/// </summary>
public sealed class WebhookDeadLetterEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [Required]
    [MaxLength(128)]
    public string HookKey { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string JobKey { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string EnvironmentTag { get; set; } = string.Empty;

    [Required]
    public string Payload { get; set; } = string.Empty;

    public string? HeadersJson { get; set; }

    public string? MetadataJson { get; set; }

    [Required]
    [MaxLength(128)]
    public string FailureReason { get; set; } = string.Empty;

    [MaxLength(2048)]
    public string? ErrorDetails { get; set; }

    public int? StatusCode { get; set; }

    public int Attempts { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime? LastAttemptAtUtc { get; set; }

    public DateTime? NextAttemptAtUtc { get; set; }

    public DateTime? ExpiresAtUtc { get; set; }
}
