using System;
using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Tenant-scoped username/password credential (self-hosted auth).
/// </summary>
public sealed class PasswordUserEntity
{
    [Key]
    [MaxLength(64)]
    public string UserId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string Username { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string UsernameNormalized { get; set; } = string.Empty;

    [Required]
    [MaxLength(1024)]
    public string PasswordHash { get; set; } = string.Empty;

    public string? ScopesJson { get; set; }

    public bool IsActive { get; set; } = true;

    public int FailedLoginCount { get; set; }

    public DateTime? LockoutEndUtc { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
