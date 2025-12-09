using System;
using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    [DbContext(typeof(SqlServerDbContext))]
    [Migration("20251209133000_AddWebhookEndpointEvents")]
    public partial class AddWebhookEndpointEvents : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.CreateTable(
                name: "WebhookEndpointEvents",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    HookKey = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EventType = table.Column<string>(type: "nvarchar(32)", maxLength: 32, nullable: false),
                    OccurredAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookEndpointEvents", x => x.Id);
                });

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
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebhookEndpointEvents",
                schema: "croniq");
        }
    }
}
