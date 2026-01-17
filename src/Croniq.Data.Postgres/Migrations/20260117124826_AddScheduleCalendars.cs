using System;
using Microsoft.EntityFrameworkCore.Migrations;
using Npgsql.EntityFrameworkCore.PostgreSQL.Metadata;

#nullable disable

namespace Croniq.Data.Postgres.Migrations
{
    /// <inheritdoc />
    public partial class AddScheduleCalendars : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "CalendarId",
                schema: "croniq",
                table: "Triggers",
                type: "character varying(128)",
                maxLength: 128,
                nullable: true);

            migrationBuilder.CreateTable(
                name: "Calendars",
                schema: "croniq",
                columns: table => new
                {
                    Id = table.Column<long>(type: "bigint", nullable: false)
                        .Annotation("Npgsql:ValueGenerationStrategy", NpgsqlValueGenerationStrategy.IdentityByDefaultColumn),
                    CalendarId = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    TenantId = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    EnvironmentTag = table.Column<string>(type: "character varying(64)", maxLength: 64, nullable: false),
                    Name = table.Column<string>(type: "character varying(256)", maxLength: 256, nullable: false),
                    Description = table.Column<string>(type: "character varying(1024)", maxLength: 1024, nullable: true),
                    TimeZoneId = table.Column<string>(type: "character varying(128)", maxLength: 128, nullable: false),
                    Mode = table.Column<int>(type: "integer", nullable: false),
                    Enabled = table.Column<bool>(type: "boolean", nullable: false),
                    RulesJson = table.Column<string>(type: "text", nullable: true),
                    CreatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())"),
                    UpdatedAtUtc = table.Column<DateTime>(type: "timestamp with time zone", nullable: false, defaultValueSql: "timezone('utc', now())")
                },
                constraints: table =>
                {
                    table.PrimaryKey("PK_Calendars", x => x.Id);
                    table.ForeignKey(
                        name: "FK_Calendars_Tenants_TenantId",
                        column: x => x.TenantId,
                        principalSchema: "croniq",
                        principalTable: "Tenants",
                        principalColumn: "TenantId",
                        onDelete: ReferentialAction.Restrict);
                });

            migrationBuilder.CreateIndex(
                name: "IX_Calendars_TenantId_EnvironmentTag",
                schema: "croniq",
                table: "Calendars",
                columns: new[] { "TenantId", "EnvironmentTag" });

            migrationBuilder.CreateIndex(
                name: "IX_Calendars_TenantId_EnvironmentTag_CalendarId",
                schema: "croniq",
                table: "Calendars",
                columns: new[] { "TenantId", "EnvironmentTag", "CalendarId" },
                unique: true);

            migrationBuilder.CreateIndex(
                name: "IX_Calendars_TenantId_EnvironmentTag_Name",
                schema: "croniq",
                table: "Calendars",
                columns: new[] { "TenantId", "EnvironmentTag", "Name" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropTable(
                name: "Calendars",
                schema: "croniq");

            migrationBuilder.DropColumn(
                name: "CalendarId",
                schema: "croniq",
                table: "Triggers");
        }
    }
}
