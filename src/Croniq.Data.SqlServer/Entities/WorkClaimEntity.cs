using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Represents the active lease for a work item.
/// </summary>
public sealed class WorkClaimEntity
{
    [Key]
    [ForeignKey(nameof(WorkItem))]
    public long WorkItemId { get; set; }

    public WorkItemEntity WorkItem { get; set; } = null!;

    [Required]
    [MaxLength(64)]
    public string LeaseId { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string RunnerId { get; set; } = string.Empty;

    public DateTime LeaseExpiresAtUtc { get; set; }

    public DateTime? LastHeartbeatAtUtc { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
