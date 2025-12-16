using Croniq.Data.SqlServer.Entities;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

namespace Croniq.Data.SqlServer;

/// <summary>
/// EF Core DbContext consolidating Croniq's persistence + auth state model.
/// </summary>
public sealed class SqlServerDbContext(DbContextOptions<SqlServerDbContext> options) : DbContext(options)
{
    public DbSet<JobEntity> Jobs => Set<JobEntity>();
    public DbSet<TriggerEntity> Triggers => Set<TriggerEntity>();
    public DbSet<DeadLetterEntity> DeadLetters => Set<DeadLetterEntity>();
    public DbSet<ApiClientEntity> ApiClients => Set<ApiClientEntity>();
    public DbSet<ApiKeyEntity> ApiKeys => Set<ApiKeyEntity>();
    public DbSet<WebhookEndpointEntity> WebhookEndpoints => Set<WebhookEndpointEntity>();
    public DbSet<WebhookEndpointEventEntity> WebhookEndpointEvents => Set<WebhookEndpointEventEntity>();
    public DbSet<WebhookDeadLetterEntity> WebhookDeadLetters => Set<WebhookDeadLetterEntity>();
    public DbSet<WebhookSecretHistoryEntity> WebhookSecretHistory => Set<WebhookSecretHistoryEntity>();
    public DbSet<WebhookEndpointIpRuleEntity> WebhookEndpointIpRules => Set<WebhookEndpointIpRuleEntity>();
    public DbSet<TenantEntity> Tenants => Set<TenantEntity>();
    public DbSet<PasswordUserEntity> PasswordUsers => Set<PasswordUserEntity>();
    public DbSet<RefreshTokenEntity> RefreshTokens => Set<RefreshTokenEntity>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        ConfigureTenants(modelBuilder.Entity<TenantEntity>());
        ConfigureJobs(modelBuilder.Entity<JobEntity>());
        ConfigureTriggers(modelBuilder.Entity<TriggerEntity>());
        ConfigureDeadLetters(modelBuilder.Entity<DeadLetterEntity>());
        ConfigureApiClients(modelBuilder.Entity<ApiClientEntity>());
        ConfigureApiKeys(modelBuilder.Entity<ApiKeyEntity>());
        ConfigureWebhookEndpoints(modelBuilder.Entity<WebhookEndpointEntity>());
        ConfigureWebhookDeadLetters(modelBuilder.Entity<WebhookDeadLetterEntity>());
        ConfigureWebhookEndpointEvents(modelBuilder.Entity<WebhookEndpointEventEntity>());
        ConfigureWebhookSecretHistory(modelBuilder.Entity<WebhookSecretHistoryEntity>());
        ConfigureWebhookEndpointIpRules(modelBuilder.Entity<WebhookEndpointIpRuleEntity>());
        ConfigurePasswordUsers(modelBuilder.Entity<PasswordUserEntity>());
        ConfigureRefreshTokens(modelBuilder.Entity<RefreshTokenEntity>());
    }

    private static void ConfigureTenants(EntityTypeBuilder<TenantEntity> builder)
    {
        builder.ToTable("Tenants", "croniq");
        builder.HasKey(x => x.TenantId);
        builder.HasIndex(x => x.Reference).IsUnique();
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureJobs(EntityTypeBuilder<JobEntity> builder)
    {
        builder.ToTable("Jobs", "croniq");
        builder.HasIndex(x => x.JobKey).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag });
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureTriggers(EntityTypeBuilder<TriggerEntity> builder)
    {
        builder.ToTable("Triggers", "croniq");
        builder.HasIndex(x => x.TriggerKey).IsUnique();
        builder.HasIndex(x => new { x.JobId, x.Enabled, x.NextFireAtUtc });
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.RowVersion).IsRowVersion();
        builder.HasOne(x => x.Job)
            .WithMany(j => j.Triggers)
            .HasForeignKey(x => x.JobId)
            .OnDelete(DeleteBehavior.Cascade);
    }

    private static void ConfigureDeadLetters(EntityTypeBuilder<DeadLetterEntity> builder)
    {
        builder.ToTable("DeadLetters", "croniq");
        builder.HasIndex(x => x.FireAtUtc);
        builder.HasIndex(x => x.ExpiresAtUtc);
        builder.HasOne(x => x.Trigger)
            .WithMany(t => t.DeadLetters)
            .HasForeignKey(x => x.TriggerId)
            .OnDelete(DeleteBehavior.Cascade);
    }

    private static void ConfigureApiClients(EntityTypeBuilder<ApiClientEntity> builder)
    {
        builder.ToTable("ApiClients", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.ClientId }).IsUnique();
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureApiKeys(EntityTypeBuilder<ApiKeyEntity> builder)
    {
        builder.ToTable("ApiKeys", "croniq");
        builder.HasIndex(x => x.KeyId).IsUnique();
        builder.HasIndex(x => new { x.IsActive, x.ExpiresAtUtc });
        builder.HasOne(x => x.Client)
            .WithMany(c => c.ApiKeys)
            .HasForeignKey(x => x.ApiClientId)
            .OnDelete(DeleteBehavior.Cascade);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookEndpoints(EntityTypeBuilder<WebhookEndpointEntity> builder)
    {
        builder.ToTable("WebhookEndpoints", "croniq");
        builder.HasIndex(x => x.HookKey).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.Enabled });
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookDeadLetters(EntityTypeBuilder<WebhookDeadLetterEntity> builder)
    {
        builder.ToTable("WebhookDeadLetters", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.CreatedAtUtc });
        builder.HasIndex(x => x.HookKey);
        builder.HasIndex(x => x.NextAttemptAtUtc);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookEndpointEvents(EntityTypeBuilder<WebhookEndpointEventEntity> builder)
    {
        builder.ToTable("WebhookEndpointEvents", "croniq");
        builder.HasIndex(x => x.OccurredAtUtc);
        builder.HasIndex(x => x.HookKey);
        builder.Property(x => x.OccurredAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookSecretHistory(EntityTypeBuilder<WebhookSecretHistoryEntity> builder)
    {
        builder.ToTable("WebhookSecretHistory", "croniq");
        builder.HasIndex(x => new { x.HookKey, x.TenantId, x.EnvironmentTag, x.ActivatedAtUtc });
        builder.HasIndex(x => new { x.HookKey, x.ExpiresAtUtc });
        builder.Property(x => x.ActivatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookEndpointIpRules(EntityTypeBuilder<WebhookEndpointIpRuleEntity> builder)
    {
        builder.ToTable("WebhookEndpointIpRules", "croniq");
        builder.HasIndex(x => new { x.HookKey, x.TenantId, x.EnvironmentTag });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag });
        builder.HasIndex(x => new { x.HookKey, x.Cidr }).IsUnique();
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigurePasswordUsers(EntityTypeBuilder<PasswordUserEntity> builder)
    {
        builder.ToTable("Users", "auth");
        builder.HasKey(x => x.UserId);
        builder.HasIndex(x => new { x.TenantId, x.UsernameNormalized }).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.IsActive });
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureRefreshTokens(EntityTypeBuilder<RefreshTokenEntity> builder)
    {
        builder.ToTable("RefreshTokens", "auth");
        builder.HasKey(x => x.TokenId);
        builder.HasIndex(x => new { x.TenantId, x.TokenHash }).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.UserId });
        builder.HasIndex(x => x.ExpiresAtUtc);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }
}
