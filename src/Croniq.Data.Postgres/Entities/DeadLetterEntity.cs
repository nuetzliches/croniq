using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

/// <summary>
/// Captures failed trigger executions for later inspection.
/// </summary>
public sealed class DeadLetterEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [ForeignKey(nameof(Trigger))]
    public long TriggerId { get; set; }

    public TriggerEntity Trigger { get; set; } = null!;

    public DateTime FireAtUtc { get; set; }

    [Required]
    [MaxLength(256)]
    public string Reason { get; set; } = string.Empty;

    [Required]
    public string Payload { get; set; } = string.Empty;

    public string? MetadataJson { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime? ExpiresAtUtc { get; set; }
}
