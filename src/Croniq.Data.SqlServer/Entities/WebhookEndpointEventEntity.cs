using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Event log for webhook endpoint mutations (used for cache invalidation/changefeed).
/// </summary>
public sealed class WebhookEndpointEventEntity
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
    [MaxLength(32)]
    public string EventType { get; set; } = string.Empty;

    public DateTime OccurredAtUtc { get; set; }
}
