using System;
using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    [DbContext(typeof(SqlServerDbContext))]
    [Migration("20251209090000_AddCroniqCoreEntities")]
    public partial class AddCroniqCoreEntities : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.CreateTable(
                name: "ApiClients",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: true),
                    ClientId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    Name = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: true),
                    ScopesJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    IsActive = table.Column<bool>(type: "bit", nullable: false),
                    IsDeleted = table.Column<bool>(type: "bit", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_ApiClients", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "Jobs",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    NamespaceSegment = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    JobKey = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    Name = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    Description = table.Column<string>(type: "nvarchar(1024)", maxLength: 1024, nullable: true),
                    Variant = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: true),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Jobs", x => x.Id);
                });

            migrationBuilder.CreateTable(
                name: "ApiKeys",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    ApiClientId = table.Column<long>(type: "bigint", nullable: false),
                    KeyId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    SecretHash = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    SecretSalt = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    ScopesJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: true),
                    IsActive = table.Column<bool>(type: "bit", nullable: false),
                    ExpiresAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()")
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
                        .Annotation("SqlServer:Identity", "1, 1"),
                    JobId = table.Column<long>(type: "bigint", nullable: false),
                    JobKey = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    TriggerKey = table.Column<string>(type: "nvarchar(512)", maxLength: 512, nullable: false),
                    CronExpression = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    TimeZoneId = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    Enabled = table.Column<bool>(type: "bit", nullable: false),
                    StartAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    EndAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    NextFireAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    LastFiredAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    LastCompletedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    LastResult = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: true),
                    LeaseId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: true),
                    LeaseInstanceId = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: true),
                    LeaseExpiresAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    IsDeleted = table.Column<bool>(type: "bit", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    RowVersion = table.Column<byte[]>(type: "rowversion", rowVersion: true, nullable: false)
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
                name: "DeadLetters",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    TriggerId = table.Column<long>(type: "bigint", nullable: false),
                    Payload = table.Column<string>(type: "nvarchar(max)", nullable: false),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    Reason = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    FireAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false),
                    ExpiresAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true)
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
                name: "IX_Jobs_JobKey",
                schema: "croniq",
                table: "Jobs",
                column: "JobKey",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_Jobs_TenantId_EnvironmentTag",
                schema: "croniq",
                table: "Jobs",
                columns: new[] { "TenantId", "EnvironmentTag" });

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
                name: "ApiClients",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Triggers",
                schema: "croniq");

            migrationBuilder.DropTable(
                name: "Jobs",
                schema: "croniq");
        }
    }
}
