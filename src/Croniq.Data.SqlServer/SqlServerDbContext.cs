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
    public DbSet<CalendarEntity> Calendars => Set<CalendarEntity>();
    public DbSet<TriggerEntity> Triggers => Set<TriggerEntity>();
    public DbSet<DeadLetterEntity> DeadLetters => Set<DeadLetterEntity>();
    public DbSet<WorkerInstanceEntity> WorkerInstances => Set<WorkerInstanceEntity>();
    public DbSet<RunnerEntity> Runners => Set<RunnerEntity>();
    public DbSet<RunnerCapabilityEntity> RunnerCapabilities => Set<RunnerCapabilityEntity>();
    public DbSet<WorkItemEntity> WorkItems => Set<WorkItemEntity>();
    public DbSet<WorkClaimEntity> WorkClaims => Set<WorkClaimEntity>();
    public DbSet<ApiClientEntity> ApiClients => Set<ApiClientEntity>();
    public DbSet<ApiKeyEntity> ApiKeys => Set<ApiKeyEntity>();
    public DbSet<WebhookEndpointEntity> WebhookEndpoints => Set<WebhookEndpointEntity>();
    public DbSet<WebhookEndpointEventEntity> WebhookEndpointEvents => Set<WebhookEndpointEventEntity>();
    public DbSet<WebhookIngressEventEntity> WebhookIngressEvents => Set<WebhookIngressEventEntity>();
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
        ConfigureCalendars(modelBuilder.Entity<CalendarEntity>());
        ConfigureTriggers(modelBuilder.Entity<TriggerEntity>());
        ConfigureDeadLetters(modelBuilder.Entity<DeadLetterEntity>());
        ConfigureWorkerInstances(modelBuilder.Entity<WorkerInstanceEntity>());
        ConfigureRunners(modelBuilder.Entity<RunnerEntity>());
        ConfigureRunnerCapabilities(modelBuilder.Entity<RunnerCapabilityEntity>());
        ConfigureWorkItems(modelBuilder.Entity<WorkItemEntity>());
        ConfigureWorkClaims(modelBuilder.Entity<WorkClaimEntity>());
        ConfigureApiClients(modelBuilder.Entity<ApiClientEntity>());
        ConfigureApiKeys(modelBuilder.Entity<ApiKeyEntity>());
        ConfigureWebhookEndpoints(modelBuilder.Entity<WebhookEndpointEntity>());
        ConfigureWebhookDeadLetters(modelBuilder.Entity<WebhookDeadLetterEntity>());
        ConfigureWebhookEndpointEvents(modelBuilder.Entity<WebhookEndpointEventEntity>());
        ConfigureWebhookIngressEvents(modelBuilder.Entity<WebhookIngressEventEntity>());
        ConfigureWebhookSecretHistory(modelBuilder.Entity<WebhookSecretHistoryEntity>());
        ConfigureWebhookEndpointIpRules(modelBuilder.Entity<WebhookEndpointIpRuleEntity>());
        ConfigurePasswordUsers(modelBuilder.Entity<PasswordUserEntity>());
        ConfigureRefreshTokens(modelBuilder.Entity<RefreshTokenEntity>());
    }

    private static void ConfigureTenants(EntityTypeBuilder<TenantEntity> builder)
    {
        builder.ToTable("Tenants", "croniq");
        builder.HasKey(x => x.TenantId);
        builder.Property(x => x.Reference).IsRequired().HasMaxLength(64);
        builder.HasIndex(x => x.Reference).IsUnique();
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureJobs(EntityTypeBuilder<JobEntity> builder)
    {
        builder.ToTable("Jobs", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.JobKey }).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
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

    private static void ConfigureWorkerInstances(EntityTypeBuilder<WorkerInstanceEntity> builder)
    {
        builder.ToTable("WorkerInstances", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.InstanceId }).IsUnique();
        builder.HasIndex(x => x.ExpiresAtUtc);
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.InstanceId).HasMaxLength(256);
        builder.Property(x => x.EnvironmentTag).HasMaxLength(64);
        builder.Property(x => x.TenantId).HasMaxLength(64);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureCalendars(EntityTypeBuilder<CalendarEntity> builder)
    {
        builder.ToTable("Calendars", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.CalendarId }).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.Name });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureRunners(EntityTypeBuilder<RunnerEntity> builder)
    {
        builder.ToTable("Runners", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.RunnerId }).IsUnique();
        builder.HasIndex(x => x.ExpiresAtUtc);
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.RunnerId).HasMaxLength(256);
        builder.Property(x => x.EnvironmentTag).HasMaxLength(64);
        builder.Property(x => x.TenantId).HasMaxLength(64);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureRunnerCapabilities(EntityTypeBuilder<RunnerCapabilityEntity> builder)
    {
        builder.ToTable("RunnerCapabilities", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.RunnerId }).IsUnique();
        builder.HasIndex(x => x.UpdatedAtUtc);
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.RunnerId).HasMaxLength(256);
        builder.Property(x => x.EnvironmentTag).HasMaxLength(64);
        builder.Property(x => x.TenantId).HasMaxLength(64);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWorkItems(EntityTypeBuilder<WorkItemEntity> builder)
    {
        builder.ToTable("WorkItems", "croniq");
        builder.HasIndex(x => x.ExecutionId).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.Status, x.CreatedAtUtc });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.JobKey });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.ExecutionId).HasMaxLength(64);
        builder.Property(x => x.JobKey).HasMaxLength(256);
        builder.Property(x => x.TriggerId).HasMaxLength(512);
        builder.Property(x => x.Status).HasMaxLength(32);
        builder.Property(x => x.EnvironmentTag).HasMaxLength(64);
        builder.Property(x => x.TenantId).HasMaxLength(64);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWorkClaims(EntityTypeBuilder<WorkClaimEntity> builder)
    {
        builder.ToTable("WorkClaims", "croniq");
        builder.HasKey(x => x.WorkItemId);
        builder.HasIndex(x => x.LeaseId).IsUnique();
        builder.HasIndex(x => x.LeaseExpiresAtUtc);
        builder.HasOne(x => x.WorkItem)
            .WithOne(w => w.Claim)
            .HasForeignKey<WorkClaimEntity>(x => x.WorkItemId)
            .OnDelete(DeleteBehavior.Cascade);
        builder.Property(x => x.LeaseId).HasMaxLength(64);
        builder.Property(x => x.RunnerId).HasMaxLength(256);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureApiClients(EntityTypeBuilder<ApiClientEntity> builder)
    {
        builder.ToTable("ApiClients", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.ClientId }).IsUnique();
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
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
        builder.HasAlternateKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.Enabled, x.IsDeleted });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.HookKey, x.IsDeleted });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookDeadLetters(EntityTypeBuilder<WebhookDeadLetterEntity> builder)
    {
        builder.ToTable("WebhookDeadLetters", "croniq");
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.CreatedAtUtc });
        builder.HasIndex(x => x.HookKey);
        builder.HasIndex(x => x.NextAttemptAtUtc);
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookEndpointEvents(EntityTypeBuilder<WebhookEndpointEventEntity> builder)
    {
        builder.ToTable("WebhookEndpointEvents", "croniq");
        builder.HasIndex(x => x.OccurredAtUtc);
        builder.HasIndex(x => x.HookKey);
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.HasOne<WebhookEndpointEntity>()
            .WithMany()
            .HasForeignKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey })
            .HasPrincipalKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey })
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.OccurredAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookIngressEvents(EntityTypeBuilder<WebhookIngressEventEntity> builder)
    {
        builder.ToTable("WebhookIngressEvents", "croniq");
        builder.HasIndex(x => x.EventId).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.Status, x.LeaseExpiresAtUtc });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.ReceivedAtUtc });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.EventId).HasMaxLength(64);
        builder.Property(x => x.HookKey).HasMaxLength(128);
        builder.Property(x => x.JobKey).HasMaxLength(256);
        builder.Property(x => x.EnvironmentTag).HasMaxLength(64);
        builder.Property(x => x.TenantId).HasMaxLength(64);
        builder.Property(x => x.Status).HasMaxLength(32);
        builder.Property(x => x.LeaseId).HasMaxLength(64);
        builder.Property(x => x.LastError).HasMaxLength(1024);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookSecretHistory(EntityTypeBuilder<WebhookSecretHistoryEntity> builder)
    {
        builder.ToTable("WebhookSecretHistory", "croniq");
        builder.HasIndex(x => new { x.HookKey, x.TenantId, x.EnvironmentTag, x.ActivatedAtUtc });
        builder.HasIndex(x => new { x.HookKey, x.ExpiresAtUtc });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.HasOne<WebhookEndpointEntity>()
            .WithMany()
            .HasForeignKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey })
            .HasPrincipalKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey })
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.ActivatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigureWebhookEndpointIpRules(EntityTypeBuilder<WebhookEndpointIpRuleEntity> builder)
    {
        builder.ToTable("WebhookEndpointIpRules", "croniq");
        builder.HasIndex(x => new { x.HookKey, x.TenantId, x.EnvironmentTag });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag });
        builder.HasIndex(x => new { x.TenantId, x.EnvironmentTag, x.HookKey, x.Cidr }).IsUnique();
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.HasOne<WebhookEndpointEntity>()
            .WithMany()
            .HasForeignKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey })
            .HasPrincipalKey(x => new { x.TenantId, x.EnvironmentTag, x.HookKey })
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
        builder.Property(x => x.UpdatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }

    private static void ConfigurePasswordUsers(EntityTypeBuilder<PasswordUserEntity> builder)
    {
        builder.ToTable("Users", "auth");
        builder.HasKey(x => x.UserId);
        builder.HasIndex(x => new { x.TenantId, x.UsernameNormalized }).IsUnique();
        builder.HasIndex(x => new { x.TenantId, x.IsActive });
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
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
        builder.HasOne<TenantEntity>()
            .WithMany()
            .HasForeignKey(x => x.TenantId)
            .OnDelete(DeleteBehavior.Restrict);
        builder.Property(x => x.CreatedAtUtc).HasDefaultValueSql("sysutcdatetime()");
    }
}
