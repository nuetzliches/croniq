using System;
using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    [DbContext(typeof(SqlServerDbContext))]
    [Migration("20251210103000_AddWebhookSecretHistory")]
    public partial class AddWebhookSecretHistory : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.CreateTable(
                name: "WebhookSecretHistory",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    HookKey = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    Secret = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    SecretHash = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    ActivatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    ExpiresAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    RotatedBy = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: true),
                    Notes = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookSecretHistory", x => x.Id);
                });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookSecretHistory_HookKey_ActivatedAtUtc",
                schema: "croniq",
                table: "WebhookSecretHistory",
                columns: new[] { "HookKey", "TenantId", "EnvironmentTag", "ActivatedAtUtc" });

            migrationBuilder.CreateIndex(
                name: "IX_WebhookSecretHistory_HookKey_ExpiresAtUtc",
                schema: "croniq",
                table: "WebhookSecretHistory",
                columns: new[] { "HookKey", "ExpiresAtUtc" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebhookSecretHistory",
                schema: "croniq");
        }
    }
}
