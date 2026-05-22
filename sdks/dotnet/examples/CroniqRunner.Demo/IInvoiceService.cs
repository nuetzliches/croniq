namespace CroniqRunner.Demo;

/// <summary>Stand-in for a real business service that yields progress events line-by-line.</summary>
public interface IInvoiceService
{
    IAsyncEnumerable<string> GenerateAsync(string customerId, CancellationToken cancellationToken);
}

internal sealed class FakeInvoiceService : IInvoiceService
{
    public async IAsyncEnumerable<string> GenerateAsync(
        string customerId,
        [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken cancellationToken)
    {
        yield return $"loading customer {customerId}";
        await Task.Delay(100, cancellationToken);
        yield return "collecting line items";
        await Task.Delay(100, cancellationToken);
        yield return "computing totals";
        await Task.Delay(100, cancellationToken);
        yield return "writing PDF";
        await Task.Delay(100, cancellationToken);
        yield return $"invoice for {customerId} ready";
    }
}
