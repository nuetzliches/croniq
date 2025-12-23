using System;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Top-level tenant metadata for Croniq admin flows.
/// </summary>
public sealed class TenantEntity
{
    [Key]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string Name { get; set; } = string.Empty;

    public bool IsActive { get; set; } = true;

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
