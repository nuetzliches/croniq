using System;
using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

public sealed class WebhookIngressEventEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [Required]
    [MaxLength(64)]
    public string EventId { get; set; } = string.Empty;

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

    public DateTime ReceivedAtUtc { get; set; }

    [MaxLength(32)]
    public string Status { get; set; } = "Pending";

    [MaxLength(64)]
    public string? LeaseId { get; set; }

    public DateTime? LeaseExpiresAtUtc { get; set; }

    public int AttemptCount { get; set; }

    [MaxLength(1024)]
    public string? LastError { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
