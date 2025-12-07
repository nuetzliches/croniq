using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Concrete API key credentials issued to clients.
/// </summary>
public sealed class ApiKeyEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [ForeignKey(nameof(Client))]
    public long ApiClientId { get; set; }

    public ApiClientEntity Client { get; set; } = null!;

    [Required]
    [MaxLength(64)]
    public string KeyId { get; set; } = string.Empty;

    [Required]
    [MaxLength(256)]
    public string SecretHash { get; set; } = string.Empty;

    [Required]
    [MaxLength(128)]
    public string SecretSalt { get; set; } = string.Empty;

    public DateTime? ExpiresAtUtc { get; set; }

    public bool IsActive { get; set; } = true;

    public string? MetadataJson { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }
}
