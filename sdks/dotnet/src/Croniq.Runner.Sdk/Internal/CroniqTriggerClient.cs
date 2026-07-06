using System.Net.Http.Json;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Default <see cref="ICroniqTriggerClient"/> over <c>POST /v1/trigger</c>.
/// Sends snake_case JSON via the source-generated
/// <see cref="CroniqJsonContext"/>. The per-request timeout comes from
/// <see cref="CroniqClientOptions.RequestTimeout"/> via a linked
/// <see cref="CancellationTokenSource"/> so the underlying
/// <see cref="HttpClient.Timeout"/> can stay infinite.
/// </summary>
internal sealed class CroniqTriggerClient(
    HttpClient http,
    IOptionsMonitor<CroniqClientOptions> options) : ICroniqTriggerClient
{
    public async Task<TriggerResult> TriggerAsync(
        string jobKey,
        IReadOnlyDictionary<string, string>? metadata = null,
        IReadOnlyList<string>? require = null,
        IReadOnlyList<string>? prefer = null,
        string? timeout = null,
        string? idempotencyKey = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(jobKey);

        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        linked.CancelAfter(options.CurrentValue.RequestTimeout);

        var request = new TriggerRequest(jobKey, metadata, require, prefer, timeout, idempotencyKey);
        using var requestMsg = new HttpRequestMessage(HttpMethod.Post, "/v1/trigger")
        {
            Content = JsonContent.Create(request, CroniqJsonContext.Default.TriggerRequest),
        };
        using var response = await http.SendAsync(requestMsg, linked.Token).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        var body = await response.Content
            .ReadFromJsonAsync(CroniqJsonContext.Default.TriggerResponse, linked.Token)
            .ConfigureAwait(false)
            ?? throw new HttpRequestException("POST /v1/trigger returned an empty body.");
        return new TriggerResult(body.ExecutionId, body.Queued, body.Deduplicated);
    }
}
