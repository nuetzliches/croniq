using System.Net.Http.Json;
using System.Text.Json;

using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// HTTP client over the Croniq Runner API. Sends snake_case JSON via the
/// source-generated <see cref="CroniqJsonContext"/>. Per-request timeouts
/// are handled by linked <see cref="CancellationTokenSource"/>s so the
/// underlying <see cref="HttpClient.Timeout"/> can stay infinite (which
/// it must, to accommodate the 35 s long-poll on <c>/v1/work/poll</c>).
/// </summary>
internal sealed class CroniqClient(HttpClient http, ILogger<CroniqClient> logger) : ICroniqClient
{
    private static readonly JsonSerializerOptions JsonOptions = CroniqJsonContext.Default.Options;

    public async Task<PollResponse> PollAsync(PollRequest request, TimeSpan timeout, CancellationToken ct)
    {
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct);
        linked.CancelAfter(timeout);

        using var requestMsg = new HttpRequestMessage(HttpMethod.Post, "/v1/work/poll")
        {
            Content = JsonContent.Create(request, CroniqJsonContext.Default.PollRequest),
        };
        using var response = await http
            .SendAsync(requestMsg, HttpCompletionOption.ResponseHeadersRead, linked.Token)
            .ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        var body = await response.Content
            .ReadFromJsonAsync(CroniqJsonContext.Default.PollResponse, linked.Token)
            .ConfigureAwait(false);
        return body ?? new PollResponse([], []);
    }

    public async Task AckAsync(AckRequest request, CancellationToken ct)
    {
        using var requestMsg = new HttpRequestMessage(HttpMethod.Post, "/v1/work/ack")
        {
            Content = JsonContent.Create(request, CroniqJsonContext.Default.AckRequest),
        };
        using var response = await http.SendAsync(requestMsg, ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task RenewAsync(RenewRequest request, CancellationToken ct)
    {
        using var requestMsg = new HttpRequestMessage(HttpMethod.Post, "/v1/work/renew")
        {
            Content = JsonContent.Create(request, CroniqJsonContext.Default.RenewRequest),
        };
        using var response = await http.SendAsync(requestMsg, ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task PushEventsAsync(string executionId, IReadOnlyList<WorkEvent> events, CancellationToken ct)
    {
        if (events.Count == 0)
        {
            return;
        }

        using var requestMsg = new HttpRequestMessage(HttpMethod.Post, $"/v1/work/{Uri.EscapeDataString(executionId)}/events")
        {
            Content = JsonContent.Create(events, CroniqJsonContext.Default.IReadOnlyListWorkEvent),
        };
        using var response = await http.SendAsync(requestMsg, ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task RegisterJobAsync(RegisterJobRequest request, CancellationToken ct)
    {
        using var requestMsg = new HttpRequestMessage(HttpMethod.Post, "/v1/jobs/register")
        {
            Content = JsonContent.Create(request, CroniqJsonContext.Default.RegisterJobRequest),
        };
        using var response = await http.SendAsync(requestMsg, ct).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        try
        {
            var body = await response.Content
                .ReadFromJsonAsync(CroniqJsonContext.Default.RegisterJobResponse, ct)
                .ConfigureAwait(false);
            if (body is { Status: "skipped_dsl_precedence" })
            {
                logger.LogInformation(
                    "Job {JobKey} is managed by the Croniqfile (DSL precedence) — schedule registration skipped",
                    body.JobKey);
            }
        }
        catch (JsonException)
        {
            // Some server versions return 200 with no body — fine.
        }
    }
}
