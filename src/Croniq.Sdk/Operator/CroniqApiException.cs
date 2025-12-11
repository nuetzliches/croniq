using System.Net;
using System.Net.Http;
using System.Text.Json;

namespace Croniq.Sdk.Operator;

/// <summary>
/// Represents an error payload returned by a Croniq HTTP endpoint.
/// </summary>
public sealed class CroniqApiException : Exception
{
    private CroniqApiException(HttpStatusCode statusCode, string? error, string? message, string? raw)
        : base(message ?? raw ?? $"Croniq API request failed ({(int)statusCode} {statusCode}).")
    {
        StatusCode = statusCode;
        Error = error;
        RawBody = raw;
    }

    public HttpStatusCode StatusCode { get; }

    public string? Error { get; }

    public string? RawBody { get; }

    public static async Task<CroniqApiException> FromResponseAsync(HttpResponseMessage response, CancellationToken cancellationToken)
    {
        _ = response ?? throw new ArgumentNullException(nameof(response));

        var body = response.Content is null
            ? null
            : await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);

        if (!string.IsNullOrWhiteSpace(body))
        {
            try
            {
                var problem = JsonSerializer.Deserialize<ProblemDetailsPayload>(body, SerializerOptions);
                if (problem is not null)
                {
                    var message = problem.Detail ?? problem.Title;
                    return new CroniqApiException(response.StatusCode, problem.Title, message, body);
                }
            }
            catch (JsonException)
            {
                // fall back to raw body text
            }
        }

        return new CroniqApiException(response.StatusCode, null, null, body);
    }

    private sealed record ProblemDetailsPayload(string? Title, string? Detail);

    private static readonly JsonSerializerOptions SerializerOptions = new(JsonSerializerDefaults.Web);
}
