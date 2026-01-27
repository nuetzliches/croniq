using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    [DbContextAttribute(typeof(SqlServerDbContext))]
    [Migration("20260126123000_AddExecutionIntentToWorkItemsAndTriggers")]
    public partial class AddExecutionIntentToWorkItemsAndTriggers : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<string>(
                name: "ExecutionMode",
                schema: "croniq",
                table: "Triggers",
                type: "nvarchar(32)",
                maxLength: 32,
                nullable: false,
                defaultValue: "normal");

            migrationBuilder.AddColumn<string>(
                name: "InvocationSource",
                schema: "croniq",
                table: "Triggers",
                type: "nvarchar(64)",
                maxLength: 64,
                nullable: false,
                defaultValue: "schedule");

            migrationBuilder.AddColumn<string>(
                name: "ExecutionMode",
                schema: "croniq",
                table: "WorkItems",
                type: "nvarchar(32)",
                maxLength: 32,
                nullable: false,
                defaultValue: "normal");

            migrationBuilder.AddColumn<string>(
                name: "InvocationSource",
                schema: "croniq",
                table: "WorkItems",
                type: "nvarchar(64)",
                maxLength: 64,
                nullable: false,
                defaultValue: "schedule");
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "ExecutionMode",
                schema: "croniq",
                table: "WorkItems");

            migrationBuilder.DropColumn(
                name: "InvocationSource",
                schema: "croniq",
                table: "WorkItems");

            migrationBuilder.DropColumn(
                name: "ExecutionMode",
                schema: "croniq",
                table: "Triggers");

            migrationBuilder.DropColumn(
                name: "InvocationSource",
                schema: "croniq",
                table: "Triggers");
        }
    }
}
