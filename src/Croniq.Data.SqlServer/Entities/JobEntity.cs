using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Represents a logical Croniq job definition (unique by JobKey).
/// </summary>
public sealed class JobEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

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
    [MaxLength(128)]
    public string NamespaceSegment { get; set; } = string.Empty;

    [Required]
    [MaxLength(128)]
    public string Name { get; set; } = string.Empty;

    [MaxLength(128)]
    public string? Variant { get; set; }

    [MaxLength(1024)]
    public string? Description { get; set; }

    public bool IsActive { get; set; } = true;

    [MaxLength(256)]
    public string? AssignedRunnerId { get; set; }

    [MaxLength(256)]
    public string? AssignedBy { get; set; }

    public DateTime? AssignedAtUtc { get; set; }

    [MaxLength(64)]
    public string? AssignmentSource { get; set; }

    [MaxLength(1024)]
    public string? AssignmentNotes { get; set; }

    public string? MetadataJson { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }

    public ICollection<TriggerEntity> Triggers { get; set; } = new List<TriggerEntity>();
}
