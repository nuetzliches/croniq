using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations
{
    /// <inheritdoc />
    public partial class AddJobActivation : Migration
    {
        /// <inheritdoc />
        protected override void Up(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.AddColumn<bool>(
                name: "IsActive",
                schema: "croniq",
                table: "Jobs",
                type: "bit",
                nullable: false,
                defaultValue: true);
        }

        /// <inheritdoc />
        protected override void Down(MigrationBuilder migrationBuilder)
        {
            migrationBuilder.DropColumn(
                name: "IsActive",
                schema: "croniq",
                table: "Jobs");
        }
    }
}
