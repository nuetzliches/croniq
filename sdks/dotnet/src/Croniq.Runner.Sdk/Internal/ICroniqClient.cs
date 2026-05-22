using Croniq.Runner.Sdk.Protocol;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Internal abstraction over the Croniq HTTP API. Lets tests substitute
/// a fake in place of the real <see cref="CroniqClient"/>.
/// </summary>
internal interface ICroniqClient
{
    Task<PollResponse> PollAsync(PollRequest request, TimeSpan timeout, CancellationToken ct);

    Task AckAsync(AckRequest request, CancellationToken ct);

    Task RenewAsync(RenewRequest request, CancellationToken ct);

    Task PushEventsAsync(string executionId, IReadOnlyList<WorkEvent> events, CancellationToken ct);

    Task RegisterJobAsync(RegisterJobRequest request, CancellationToken ct);
}
