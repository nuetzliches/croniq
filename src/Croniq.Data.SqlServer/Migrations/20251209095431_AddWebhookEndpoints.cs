using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    public partial class AddWebhookEndpoints : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.CreateTable(
                name: "WebhookEndpoints",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    HookKey = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    JobKey = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    Secret = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    SecretHash = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    SignatureVersion = table.Column<int>(type: "int", nullable: false),
                    RequestsPerMinute = table.Column<int>(type: "int", nullable: false),
                    Enabled = table.Column<bool>(type: "bit", nullable: false),
                    RequireSignature = table.Column<bool>(type: "bit", nullable: false),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookEndpoints", x => x.Id);
                });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpoints_HookKey",
                schema: "croniq",
                table: "WebhookEndpoints",
                column: "HookKey",
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_WebhookEndpoints_TenantId_EnvironmentTag_Enabled",
                schema: "croniq",
                table: "WebhookEndpoints",
                columns: new[] { "TenantId", "EnvironmentTag", "Enabled" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebhookEndpoints",
                schema: "croniq");
        }
    }
}
