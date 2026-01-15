using System;
using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

public sealed class WebhookSecretHistoryEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [Required]
    [MaxLength(128)]
    public string HookKey { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string EnvironmentTag { get; set; } = string.Empty;

    [Required]
    [MaxLength(2048)]
    public string Secret { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string SecretHash { get; set; } = string.Empty;

    public DateTime ActivatedAtUtc { get; set; }

    public DateTime? ExpiresAtUtc { get; set; }

    [MaxLength(128)]
    public string? RotatedBy { get; set; }

    [MaxLength(256)]
    public string? Notes { get; set; }
}
