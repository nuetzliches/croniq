using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    public partial class AddWebhookDeadLetters : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.EnsureSchema(
                name: "croniq");

            migrationBuilder.CreateTable(
                name: "WebhookDeadLetters",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("SqlServer:Identity", "1, 1"),
                    HookKey = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    JobKey = table.Column<string>(type: "nvarchar(256)", maxLength: 256, nullable: false),
                    TenantId = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "nvarchar(64)", maxLength: 64, nullable: false),
                    Payload = table.Column<string>(type: "nvarchar(max)", nullable: false),
                    HeadersJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    MetadataJson = table.Column<string>(type: "nvarchar(max)", nullable: true),
                    FailureReason = table.Column<string>(type: "nvarchar(128)", maxLength: 128, nullable: false),
                    ErrorDetails = table.Column<string>(type: "nvarchar(2048)", maxLength: 2048, nullable: true),
                    StatusCode = table.Column<int>(type: "int", nullable: true),
                    Attempts = table.Column<int>(type: "int", nullable: false),
                    CreatedAtUtc = table.Column<DateTime>(type: "datetime2", nullable: false, defaultValueSql: "sysutcdatetime()"),
                    LastAttemptAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    NextAttemptAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true),
                    ExpiresAtUtc = table.Column<DateTime>(type: "datetime2", nullable: true)
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_WebhookDeadLetters", x => x.Id);
                });

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
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "WebhookDeadLetters",
                schema: "croniq");
        }
    }
}
