using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

/// <summary>
/// Represents a durable work item created from a scheduled execution.
/// </summary>
public sealed class WorkItemEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long WorkItemId { get; set; }

    [Required]
    [MaxLength(64)]
    public string ExecutionId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string EnvironmentTag { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string JobKey { get; set; } = string.Empty;

    [MaxLength(512)]
    public string? TriggerId { get; set; }

    public int Attempt { get; set; }

    [MaxLength(32)]
    public string Status { get; set; } = "queued";

    public string? PayloadJson { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }

    public WorkClaimEntity? Claim { get; set; }
}
