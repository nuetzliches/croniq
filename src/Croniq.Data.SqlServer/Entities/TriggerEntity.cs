using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Represents a scheduled trigger that enqueues executions for a job.
/// </summary>
public sealed class TriggerEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [Required]
    [MaxLength(512)]
    public string TriggerKey { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string JobKey { get; set; } = string.Empty;

    [ForeignKey(nameof(Job))]
    public long JobId { get; set; }

    public JobEntity Job { get; set; } = null!;

    [Required]
    [MaxLength(256)]
    public string CronExpression { get; set; } = string.Empty;

    [Required]
    [MaxLength(128)]
    public string TimeZoneId { get; set; } = "UTC";

    [MaxLength(128)]
    public string? CalendarId { get; set; }

    public DateTime? StartAtUtc { get; set; }

    public DateTime? EndAtUtc { get; set; }

    public bool Enabled { get; set; } = true;

    public DateTime? NextFireAtUtc { get; set; }

    public string? MetadataJson { get; set; }

    [MaxLength(32)]
    public string ExecutionMode { get; set; } = "normal";

    [MaxLength(64)]
    public string InvocationSource { get; set; } = "schedule";

    [MaxLength(64)]
    public string? LeaseId { get; set; }

    [MaxLength(128)]
    public string? LeaseInstanceId { get; set; }

    public DateTime? LeaseExpiresAtUtc { get; set; }

    public DateTime? LastFiredAtUtc { get; set; }

    public DateTime? LastCompletedAtUtc { get; set; }

    [MaxLength(256)]
    public string? LastResult { get; set; }

    public bool IsDeleted { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }

    [Timestamp]
    public byte[] RowVersion { get; set; } = Array.Empty<byte>();

    public ICollection<DeadLetterEntity> DeadLetters { get; set; } = new List<DeadLetterEntity>();
}
