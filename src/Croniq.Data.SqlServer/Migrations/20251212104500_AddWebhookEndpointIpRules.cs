using System;
using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    [DbContext(typeof(SqlServerDbContext))]
    [Migration("20251212104500_AddWebhookEndpointIpRules")]
    public partial class AddWebhookEndpointIpRules : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.CreateTable(
                name: "WebhookEndpointIpRules",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    HookKey = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    Cidr = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    Description = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: true),
                    CreatedBy = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    IsDeleted = table.Column<bool>(type: "bit", nullable: false)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookEndpointIpRules", x => x.Id);
                });

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
                name: "IX_WebhookEndpointIpRules_HookKey_Cidr",
                schema: "croniq",
                table: "WebhookEndpointIpRules",
                columns: new[] { "HookKey", "Cidr" },
                unique: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebhookEndpointIpRules",
                schema: "croniq");
        }
    }
}
