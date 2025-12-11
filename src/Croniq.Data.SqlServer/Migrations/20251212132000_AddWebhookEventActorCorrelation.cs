using Croniq.Data.SqlServer;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;

#nullable disable

namespace Croniq.Data.SqlServer.Migrations;

[DbContext(typeof(SqlServerDbContext))]
[Migration("20251212132000_AddWebhookEventActorCorrelation")]
public partial class AddWebhookEventActorCorrelation : Migration
{
    protected override void Up(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.AddColumn<string>(
            name: "Actor",
            schema: "croniq",
            table: "WebhookEndpointEvents",
            type: "nvarchar(128)",
            maxLength: 128,
            nullable: true);

        migrationBuilder.AddColumn<string>(
            name: "CorrelationId",
            schema: "croniq",
            table: "WebhookEndpointEvents",
            type: "nvarchar(64)",
            maxLength: 64,
            nullable: true);
    }

    protected override void Down(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.DropColumn(
            name: "Actor",
            schema: "croniq",
            table: "WebhookEndpointEvents");

        migrationBuilder.DropColumn(
            name: "CorrelationId",
            schema: "croniq",
            table: "WebhookEndpointEvents");
    }
}
