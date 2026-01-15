using System;
using Microsoft.EntityFrameworkCore.Migrations;
using Npgsql.EntityFrameworkCore.PostgreSQL.Metadata;

#nullable disable

namespace Croniq.Data.Postgres.Migrations
{
    /// <inheritdoc />
    public partial class InitialCreate : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.EnsureSchema(
                name: "auth");

            migrationBuilder.CreateTable(
                name: "Tenants",
                schema: "croniq",
                columns: table => new
                {
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Reference = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Name = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    IsActive = table.Column<bool>(type: "boolean", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Tenants", x => x.TenantId);
                });

            migrationBuilder.CreateTable(
                name: "ApiClients",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    ClientId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Name = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: true),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: true),
                    ScopesJson = table.Column<string>(type: "text", nullable: true),
                    IsActive = table.Column<bool>(type: "boolean", nullable: false),
                    IsDeleted = table.Column<bool>(type: "boolean", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_ApiClients", x => x.Id);
                    table.ForeignKey(
                        name: "FK_ApiClients_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "Jobs",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    JobKey = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    NamespaceSegment = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    Name = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    Variant = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: true),
                    Description = table.Column<string>(type: "character varying(1024)", maxLength: 1024, nullable: true),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Jobs", x => x.Id);
                    table.ForeignKey(
                        name: "FK_Jobs_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "RefreshTokens",
                schema: "auth",
                columns: table => new
                {
                    TokenId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    UserId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    TokenHash = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    RevokedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    ReplacedByTokenId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_RefreshTokens", x => x.TokenId);
                    table.ForeignKey(
                        name: "FK_RefreshTokens_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "RunnerCapabilities",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    RunnerId = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    CapabilitiesJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_RunnerCapabilities", x => x.Id);
                    table.ForeignKey(
                        name: "FK_RunnerCapabilities_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "Runners",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    RunnerId = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    LastSeenAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Runners", x => x.Id);
                    table.ForeignKey(
                        name: "FK_Runners_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "Users",
                schema: "auth",
                columns: table => new
                {
                    UserId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Username = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    UsernameNormalized = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    PasswordHash = table.Column<string>(type: "character varying(1024)", maxLength: 1024, nullable: false),
                    ScopesJson = table.Column<string>(type: "text", nullable: true),
                    IsActive = table.Column<bool>(type: "boolean", nullable: false),
                    PasswordChangeRequired = table.Column<bool>(type: "boolean", nullable: false),
                    FailedLoginCount = table.Column<int>(type: "integer", nullable: false),
                    LockoutEndUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Users", x => x.UserId);
                    table.ForeignKey(
                        name: "FK_Users_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WebhookDeadLetters",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    HookKey = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    JobKey = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Payload = table.Column<string>(type: "text", nullable: false),
                    HeadersJson = table.Column<string>(type: "text", nullable: true),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    FailureReason = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    ErrorDetails = table.Column<string>(type: "character varying(2048)", maxLength: 2048, nullable: true),
                    StatusCode = table.Column<int>(type: "integer", nullable: true),
                    Attempts = table.Column<int>(type: "integer", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    LastAttemptAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    NextAttemptAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookDeadLetters", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WebhookDeadLetters_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WebhookEndpoints",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    HookKey = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    JobKey = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    Secret = table.Column<string>(type: "character varying(2048)", maxLength: 2048, nullable: false),
                    SecretHash = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    SignatureVersion = table.Column<int>(type: "integer", nullable: false),
                    RequestsPerMinute = table.Column<int>(type: "integer", nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false),
                    RequireSignature = table.Column<bool>(type: "boolean", nullable: false),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    IsDeleted = table.Column<bool>(type: "boolean", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookEndpoints", x => x.Id);
                    table.UniqueConstraint("AK_WebhookEndpoints_TenantId_EnvironmentTag_HookKey", x => new { x.TenantId, x.EnvironmentTag, x.HookKey });
                    table.ForeignKey(
                        name: "FK_WebhookEndpoints_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WebhookIngressEvents",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    EventId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    HookKey = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    JobKey = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Payload = table.Column<string>(type: "text", nullable: false),
                    HeadersJson = table.Column<string>(type: "text", nullable: true),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    ReceivedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Status = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    LeaseId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: true),
                    LeaseExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    AttemptCount = table.Column<int>(type: "integer", nullable: false),
                    LastError = table.Column<string>(type: "character varying(1024)", maxLength: 1024, nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookIngressEvents", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WebhookIngressEvents_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WorkerInstances",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    InstanceId = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    LastSeenAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WorkerInstances", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WorkerInstances_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WorkItems",
                schema: "croniq",
                columns: table => new
                {
                    WorkItemId = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    ExecutionId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    JobKey = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    TriggerId = table.Column<string>(type: "character varying(512)", maxLength: 512, nullable: true),
                    Attempt = table.Column<int>(type: "integer", nullable: false),
                    Status = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    PayloadJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WorkItems", x => x.WorkItemId);
                    table.ForeignKey(
                        name: "FK_WorkItems_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "ApiKeys",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    ApiClientId = table.Column<long>(type: "bigint", nullable: false),
                    KeyId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    SecretHash = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    SecretSalt = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: true),
                    ScopesJson = table.Column<string>(type: "text", nullable: true),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    IsActive = table.Column<bool>(type: "boolean", nullable: false),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_ApiKeys", x => x.Id);
                    table.ForeignKey(
                        name: "FK_ApiKeys_ApiClients_ApiClientId",
                        column: x => x.ApiClientId,
                        principalSchema: "croniq",
                        principalTable: "ApiClients",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateTable(
                name: "Triggers",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    TriggerKey = table.Column<string>(type: "character varying(512)", maxLength: 512, nullable: false),
                    JobKey = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    JobId = table.Column<long>(type: "bigint", nullable: false),
                    CronExpression = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    TimeZoneId = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    StartAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    EndAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false),
                    NextFireAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    LeaseId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: true),
                    LeaseInstanceId = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: true),
                    LeaseExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    LastFiredAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    LastCompletedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    LastResult = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: true),
                    IsDeleted = table.Column<bool>(type: "boolean", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    xmin = table.Column<uint>(type: "xid", rowVersion: true, nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Triggers", x => x.Id);
                    table.ForeignKey(
                        name: "FK_Triggers_Jobs_JobId",
                        column: x => x.JobId,
                        principalSchema: "croniq",
                        principalTable: "Jobs",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateTable(
                name: "WebhookEndpointEvents",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    HookKey = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EventType = table.Column<string>(type: "character varying(32)", maxLength: 32, nullable: false),
                    OccurredAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    Actor = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: true),
                    CorrelationId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookEndpointEvents", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WebhookEndpointEvents_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                    table.ForeignKey(
                        name: "FK_WebhookEndpointEvents_WebhookEndpoints_TenantId_Environment~",
                        columns: x => new { x.TenantId, x.EnvironmentTag, x.HookKey },
                        principalSchema: "croniq",
                        principalTable: "WebhookEndpoints",
                        principalColumns: new[] { "TenantId", "EnvironmentTag", "HookKey" },
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WebhookEndpointIpRules",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    HookKey = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Cidr = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Description = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: true),
                    CreatedBy = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    IsDeleted = table.Column<bool>(type: "boolean", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookEndpointIpRules", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WebhookEndpointIpRules_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                    table.ForeignKey(
                        name: "FK_WebhookEndpointIpRules_WebhookEndpoints_TenantId_Environmen~",
                        columns: x => new { x.TenantId, x.EnvironmentTag, x.HookKey },
                        principalSchema: "croniq",
                        principalTable: "WebhookEndpoints",
                        principalColumns: new[] { "TenantId", "EnvironmentTag", "HookKey" },
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WebhookSecretHistory",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    HookKey = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Secret = table.Column<string>(type: "character varying(2048)", maxLength: 2048, nullable: false),
                    SecretHash = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    ActivatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    RotatedBy = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: true),
                    Notes = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookSecretHistory", x => x.Id);
                    table.ForeignKey(
                        name: "FK_WebhookSecretHistory_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                    table.ForeignKey(
                        name: "FK_WebhookSecretHistory_WebhookEndpoints_TenantId_EnvironmentT~",
                        columns: x => new { x.TenantId, x.EnvironmentTag, x.HookKey },
                        principalSchema: "croniq",
                        principalTable: "WebhookEndpoints",
                        principalColumns: new[] { "TenantId", "EnvironmentTag", "HookKey" },
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateTable(
                name: "WorkClaims",
                schema: "croniq",
                columns: table => new
                {
                    WorkItemId = table.Column<long>(type: "bigint", nullable: false),
                    LeaseId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    RunnerId = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    LeaseExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    LastHeartbeatAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WorkClaims", x => x.WorkItemId);
                    table.ForeignKey(
                        name: "FK_WorkClaims_WorkItems_WorkItemId",
                        column: x => x.WorkItemId,
                        principalSchema: "croniq",
                        principalTable: "WorkItems",
                        principalColumn: "WorkItemId",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateTable(
                name: "DeadLetters",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    TriggerId = table.Column<long>(type: "bigint", nullable: false),
                    FireAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    Reason = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    Payload = table.Column<string>(type: "text", nullable: false),
                    MetadataJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false),
                    ExpiresAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_DeadLetters", x => x.Id);
                    table.ForeignKey(
                        name: "FK_DeadLetters_Triggers_TriggerId",
                        column: x => x.TriggerId,
                        principalSchema: "croniq",
                        principalTable: "Triggers",
                        principalColumn: "Id",
                        onDelete: ReferentialAction.Cascade);
                });

            migrationBuilder.CreateIndex(
                name: "IX_ApiClients_TenantId_ClientId",
                schema: "croniq",
                table: "ApiClients",
                columns: new[] { "TenantId", "ClientId" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_ApiKeys_ApiClientId",
                schema: "croniq",
                table: "ApiKeys",
                column: "ApiClientId");

            migrationBuilder.CreateIndex(
                name: "IX_ApiKeys_IsActive_ExpiresAtUtc",
                schema: "croniq",
                table: "ApiKeys",
                columns: new[] { "IsActive", "ExpiresAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_ApiKeys_KeyId",
                schema: "croniq",
                table: "ApiKeys",
                column: "KeyId",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_DeadLetters_ExpiresAtUtc",
                schema: "croniq",
                table: "DeadLetters",
                column: "ExpiresAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_DeadLetters_FireAtUtc",
                schema: "croniq",
                table: "DeadLetters",
                column: "FireAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_DeadLetters_TriggerId",
                schema: "croniq",
                table: "DeadLetters",
                column: "TriggerId");

            migrationBuilder.CreateIndex(
                name: "IX_Jobs_TenantId_EnvironmentTag",
                schema: "croniq",
                table: "Jobs",
                columns: new[] { "TenantId", "EnvironmentTag" });

            migrationBuilder.CreateIndex(
                name: "IX_Jobs_TenantId_EnvironmentTag_JobKey",
                schema: "croniq",
                table: "Jobs",
                columns: new[] { "TenantId", "EnvironmentTag", "JobKey" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_RefreshTokens_ExpiresAtUtc",
                schema: "auth",
                table: "RefreshTokens",
                column: "ExpiresAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_RefreshTokens_TenantId_TokenHash",
                schema: "auth",
                table: "RefreshTokens",
                columns: new[] { "TenantId", "TokenHash" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_RefreshTokens_TenantId_UserId",
                schema: "auth",
                table: "RefreshTokens",
                columns: new[] { "TenantId", "UserId" });

            migrationBuilder.CreateIndex(
                name: "IX_RunnerCapabilities_TenantId_EnvironmentTag_RunnerId",
                schema: "croniq",
                table: "RunnerCapabilities",
                columns: new[] { "TenantId", "EnvironmentTag", "RunnerId" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_RunnerCapabilities_UpdatedAtUtc",
                schema: "croniq",
                table: "RunnerCapabilities",
                column: "UpdatedAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_Runners_ExpiresAtUtc",
                schema: "croniq",
                table: "Runners",
                column: "ExpiresAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_Runners_TenantId_EnvironmentTag_RunnerId",
                schema: "croniq",
                table: "Runners",
                columns: new[] { "TenantId", "EnvironmentTag", "RunnerId" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_Tenants_Reference",
                schema: "croniq",
                table: "Tenants",
                column: "Reference",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_Triggers_JobId_Enabled_NextFireAtUtc",
                schema: "croniq",
                table: "Triggers",
                columns: new[] { "JobId", "Enabled", "NextFireAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_Triggers_TriggerKey",
                schema: "croniq",
                table: "Triggers",
                column: "TriggerKey",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_Users_TenantId_IsActive",
                schema: "auth",
                table: "Users",
                columns: new[] { "TenantId", "IsActive" });

            migrationBuilder.CreateIndex(
                name: "IX_Users_TenantId_UsernameNormalized",
                schema: "auth",
                table: "Users",
                columns: new[] { "TenantId", "UsernameNormalized" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WebhookDeadLetters_HookKey",
                schema: "croniq",
                table: "WebhookDeadLetters",
                column: "HookKey");

            migrationBuilder.CreateIndex(
                name: "IX_WebhookDeadLetters_NextAttemptAtUtc",
                schema: "croniq",
                table: "WebhookDeadLetters",
                column: "NextAttemptAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_WebhookDeadLetters_TenantId_EnvironmentTag_CreatedAtUtc",
                schema: "croniq",
                table: "WebhookDeadLetters",
                columns: new[] { "TenantId", "EnvironmentTag", "CreatedAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpointEvents_HookKey",
                schema: "croniq",
                table: "WebhookEndpointEvents",
                column: "HookKey");

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpointEvents_OccurredAtUtc",
                schema: "croniq",
                table: "WebhookEndpointEvents",
                column: "OccurredAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpointEvents_TenantId_EnvironmentTag_HookKey",
                schema: "croniq",
                table: "WebhookEndpointEvents",
                columns: new[] { "TenantId", "EnvironmentTag", "HookKey" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpointIpRules_HookKey_TenantId_EnvironmentTag",
                schema: "croniq",
                table: "WebhookEndpointIpRules",
                columns: new[] { "HookKey", "TenantId", "EnvironmentTag" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpointIpRules_TenantId_EnvironmentTag",
                schema: "croniq",
                table: "WebhookEndpointIpRules",
                columns: new[] { "TenantId", "EnvironmentTag" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpointIpRules_TenantId_EnvironmentTag_HookKey_Cidr",
                schema: "croniq",
                table: "WebhookEndpointIpRules",
                columns: new[] { "TenantId", "EnvironmentTag", "HookKey", "Cidr" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpoints_TenantId_EnvironmentTag_Enabled_IsDeleted",
                schema: "croniq",
                table: "WebhookEndpoints",
                columns: new[] { "TenantId", "EnvironmentTag", "Enabled", "IsDeleted" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpoints_TenantId_EnvironmentTag_HookKey_IsDeleted",
                schema: "croniq",
                table: "WebhookEndpoints",
                columns: new[] { "TenantId", "EnvironmentTag", "HookKey", "IsDeleted" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookIngressEvents_EventId",
                schema: "croniq",
                table: "WebhookIngressEvents",
                column: "EventId",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WebhookIngressEvents_TenantId_EnvironmentTag_ReceivedAtUtc",
                schema: "croniq",
                table: "WebhookIngressEvents",
                columns: new[] { "TenantId", "EnvironmentTag", "ReceivedAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookIngressEvents_TenantId_EnvironmentTag_Status_LeaseEx~",
                schema: "croniq",
                table: "WebhookIngressEvents",
                columns: new[] { "TenantId", "EnvironmentTag", "Status", "LeaseExpiresAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookSecretHistory_HookKey_ExpiresAtUtc",
                schema: "croniq",
                table: "WebhookSecretHistory",
                columns: new[] { "HookKey", "ExpiresAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookSecretHistory_HookKey_TenantId_EnvironmentTag_Activa~",
                schema: "croniq",
                table: "WebhookSecretHistory",
                columns: new[] { "HookKey", "TenantId", "EnvironmentTag", "ActivatedAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookSecretHistory_TenantId_EnvironmentTag_HookKey",
                schema: "croniq",
                table: "WebhookSecretHistory",
                columns: new[] { "TenantId", "EnvironmentTag", "HookKey" });

            migrationBuilder.CreateIndex(
                name: "IX_WorkClaims_LeaseExpiresAtUtc",
                schema: "croniq",
                table: "WorkClaims",
                column: "LeaseExpiresAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_WorkClaims_LeaseId",
                schema: "croniq",
                table: "WorkClaims",
                column: "LeaseId",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WorkerInstances_ExpiresAtUtc",
                schema: "croniq",
                table: "WorkerInstances",
                column: "ExpiresAtUtc");

            migrationBuilder.CreateIndex(
                name: "IX_WorkerInstances_TenantId_EnvironmentTag_InstanceId",
                schema: "croniq",
                table: "WorkerInstances",
                columns: new[] { "TenantId", "EnvironmentTag", "InstanceId" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WorkItems_ExecutionId",
                schema: "croniq",
                table: "WorkItems",
                column: "ExecutionId",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WorkItems_TenantId_EnvironmentTag_JobKey",
                schema: "croniq",
                table: "WorkItems",
                columns: new[] { "TenantId", "EnvironmentTag", "JobKey" });

            migrationBuilder.CreateIndex(
                name: "IX_WorkItems_TenantId_EnvironmentTag_Status_CreatedAtUtc",
                schema: "croniq",
                table: "WorkItems",
                columns: new[] { "TenantId", "EnvironmentTag", "Status", "CreatedAtUtc" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "ApiKeys",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "DeadLetters",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "RefreshTokens",
                schema: "auth");

            migrationBuilder.DropTable(
                name: "RunnerCapabilities",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Runners",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Users",
                schema: "auth");

            migrationBuilder.DropTable(
                name: "WebhookDeadLetters",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WebhookEndpointEvents",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WebhookEndpointIpRules",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WebhookIngressEvents",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WebhookSecretHistory",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WorkClaims",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WorkerInstances",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "ApiClients",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Triggers",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WebhookEndpoints",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "WorkItems",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Jobs",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Tenants",
                schema: "croniq");
        }
    }
}
