using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

public sealed class WebhookEndpointEntity
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
    [MaxLength(256)]
    public string JobKey { get; set; } = string.Empty;

    [Required]
    [MaxLength(2048)]
    public string Secret { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string SecretHash { get; set; } = string.Empty;

    public int SignatureVersion { get; set; } = 1;

    public int RequestsPerMinute { get; set; }

    public bool Enabled { get; set; } = true;

    public bool RequireSignature { get; set; } = true;

    public string? MetadataJson { get; set; }

    public bool IsDeleted { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
