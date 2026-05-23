namespace Croniq.Runner.Sdk;

/// <summary>
/// Strongly-typed job handler. Register with
/// <c>services.AddCroniqJob&lt;THandler&gt;("job:key")</c>.
/// </summary>
public interface ICroniqJobHandler
{
    /// <summary>
    /// Handle one execution. Throw to signal failure; return normally for
    /// success. <paramref name="cancellationToken"/> fires on host shutdown
    /// or when the Croniq server requests cancellation of this execution.
    /// </summary>
    Task HandleAsync(CroniqExecutionContext context, CancellationToken cancellationToken);
}
