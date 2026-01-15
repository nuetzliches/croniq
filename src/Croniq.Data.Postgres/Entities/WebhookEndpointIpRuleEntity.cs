using System;
using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

public sealed class WebhookEndpointIpRuleEntity
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
    [MaxLength(64)]
    public string Cidr { get; set; } = string.Empty;

    [MaxLength(256)]
    public string? Description { get; set; }

    [MaxLength(128)]
    public string? CreatedBy { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }

    public bool IsDeleted { get; set; }
}
