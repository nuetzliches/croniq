using System;
using Microsoft.EntityFrameworkCore.Migrations;
using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore.Infrastructure;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    [DbContext(typeof(SqlServerDbContext))]
    [Migration("20260107234527_AddWebhookIngressEvents")]
    public partial class AddWebhookIngressEvents : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.CreateTable(
                name: "WebhookIngressEvents",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    EventId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    HookKey = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    JobKey = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    Payload = table.Column<string>(type: "nvarchar(max)", nullable: false),
                    HeadersJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    ReceivedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false),
                    Status = table.Column<string>(type: "nvarchar(32)", maxLength: 32, nullable: false),
                    LeaseId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: true),
                    LeaseExpiresAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    AttemptCount = table.Column<int>(type: "int", nullable: false),
                    LastError = table.Column<string>(type: "nvarchar(1024)", maxLength: 1024, nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()")
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
                name: "IX_WebhookIngressEvents_TenantId_EnvironmentTag_Status_LeaseExpiresAtUtc",
                schema: "croniq",
                table: "WebhookIngressEvents",
                columns: new[] { "TenantId", "EnvironmentTag", "Status", "LeaseExpiresAtUtc" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebhookIngressEvents",
                schema: "croniq");
        }
    }
}
