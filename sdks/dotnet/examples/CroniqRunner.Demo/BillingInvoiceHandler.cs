using Croniq.Runner.Sdk;

using Microsoft.Extensions.Logging;

namespace CroniqRunner.Demo;

/// <summary>
/// Example DI-friendly handler. Streams progress back to the Croniq UI
/// via the lazy <see cref="CroniqExecutionContext.LogWriter"/>, honors the
/// cancellation token, and treats job metadata as a typed JSON payload.
/// </summary>
public sealed class BillingInvoiceHandler(
    ILogger<BillingInvoiceHandler> logger,
    IInvoiceService invoices) : ICroniqJobHandler
{
    public async Task HandleAsync(CroniqExecutionContext context, CancellationToken cancellationToken)
    {
        var customerId = context.Metadata.TryGetProperty("customer_id", out var v) && v.ValueKind == System.Text.Json.JsonValueKind.String
            ? v.GetString()!
            : "demo-customer";

        logger.LogInformation(
            "generating invoice for customer {Customer} (execution {Execution}, attempt {Attempt})",
            customerId, context.ExecutionId, context.Attempt);

        await using var writer = context.LogWriter;
        await foreach (var line in invoices.GenerateAsync(customerId, cancellationToken))
        {
            await writer.WriteAsync(LogLevel.Information, line, cancellationToken: cancellationToken);
        }
    }
}
