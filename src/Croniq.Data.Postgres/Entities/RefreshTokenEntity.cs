using System;
using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.Postgres.Entities;

/// <summary>
/// Opaque refresh token stored as a hash for password-auth sessions.
/// </summary>
public sealed class RefreshTokenEntity
{
    [Key]
    [MaxLength(64)]
    public string TokenId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string UserId { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string TokenHash { get; set; } = string.Empty;

    public DateTime ExpiresAtUtc { get; set; }

    public DateTime? RevokedAtUtc { get; set; }

    [MaxLength(64)]
    public string? ReplacedByTokenId { get; set; }

    public DateTime CreatedAtUtc { get; set; }
}
