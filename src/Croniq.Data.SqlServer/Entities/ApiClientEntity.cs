using System.ComponentModel.DataAnnotations;
using System.ComponentModel.DataAnnotations.Schema;

namespace Croniq.Data.SqlServer.Entities;

/// <summary>
/// Metadata for machine-to-machine clients allowed to issue Croniq API keys.
/// </summary>
public sealed class ApiClientEntity
{
    [Key]
    [DatabaseGenerated(DatabaseGeneratedOption.Identity)]
    public long Id { get; set; }

    [Required]
    [MaxLength(64)]
    public string TenantId { get; set; } = string.Empty;

    [Required]
    [MaxLength(64)]
    public string ClientId { get; set; } = string.Empty;

    [MaxLength(256)]
    public string? Name { get; set; }

    [MaxLength(64)]
    public string? EnvironmentTag { get; set; }

    public string? ScopesJson { get; set; }

    public bool IsActive { get; set; } = true;

    public bool IsDeleted { get; set; }

    public DateTime CreatedAtUtc { get; set; }

    public DateTime UpdatedAtUtc { get; set; }

    public ICollection<ApiKeyEntity> ApiKeys { get; set; } = new List<ApiKeyEntity>();
}
