using System;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.Postgres.Migrations
{
    /// <inheritdoc />
    public partial class AddJobAssignment : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<DateTime>(
                name: "AssignedAtUtc",
                schema: "croniq",
                table: "Jobs",
                type: "timestamp with time zone",
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "AssignedBy",
                schema: "croniq",
                table: "Jobs",
                type: "character varying(256)",
                maxLength: 256,
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "AssignedRunnerId",
                schema: "croniq",
                table: "Jobs",
                type: "character varying(256)",
                maxLength: 256,
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "AssignmentNotes",
                schema: "croniq",
                table: "Jobs",
                type: "character varying(1024)",
                maxLength: 1024,
                nullable: true);

            migrationBuilder.AddColumn<string>(
                name: "AssignmentSource",
                schema: "croniq",
                table: "Jobs",
                type: "character varying(64)",
                maxLength: 64,
                nullable: true);

            migrationBuilder.CreateIndex(
                name: "IX_Jobs_TenantId_EnvironmentTag_AssignedRunnerId",
                schema: "croniq",
                table: "Jobs",
                columns: new[] { "TenantId", "EnvironmentTag", "AssignedRunnerId" });
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropIndex(
                name: "IX_Jobs_TenantId_EnvironmentTag_AssignedRunnerId",
                schema: "croniq",
                table: "Jobs");

            migrationBuilder.DropColumn(
                name: "AssignedAtUtc",
                schema: "croniq",
                table: "Jobs");

            migrationBuilder.DropColumn(
                name: "AssignedBy",
                schema: "croniq",
                table: "Jobs");

            migrationBuilder.DropColumn(
                name: "AssignedRunnerId",
                schema: "croniq",
                table: "Jobs");

            migrationBuilder.DropColumn(
                name: "AssignmentNotes",
                schema: "croniq",
                table: "Jobs");

            migrationBuilder.DropColumn(
                name: "AssignmentSource",
                schema: "croniq",
                table: "Jobs");
        }
    }
}
