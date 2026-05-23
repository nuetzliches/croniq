using System.Text.Json;

using WireMock.RequestBuilders;
using WireMock.ResponseBuilders;
using WireMock.ResponseProviders;
using WireMock.Server;
using WireMock.Settings;
using WireMock.Types;
using WireMock.Util;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Wraps a WireMock.Net server scripted from a <see cref="CaseSpec"/>'s
/// <c>server_script</c>. Sequential matching with <c>match_count</c> support
/// is implemented in-process: a single mapping per (method, path) group
/// delegates to a callback that picks the right rule for the current hit.
/// </summary>
internal sealed class MockServerHarness : IAsyncDisposable
{
    private readonly WireMockServer _server;
    private readonly Dictionary<string, int> _hits = new(StringComparer.Ordinal);
    private readonly object _lock = new();
    private static readonly JsonSerializerOptions _jsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = false,
    };

    public string BaseUrl => _server.Urls[0];

    public MockServerHarness(IEnumerable<ScriptEntrySpec> script)
    {
        _server = WireMockServer.Start(new WireMockServerSettings { Port = 0, StartTimeout = 5000 });

        foreach (var group in script.GroupBy(e => (e.Method, e.Path)))
        {
            var key = group.Key;
            var entries = group.OrderBy(e => e.MatchCount ?? int.MaxValue).ToList();

            _server
                .Given(Request.Create().WithPath(key.Path).UsingMethod(key.Method))
                .RespondWith(new SequentialResponseProvider(this, key, entries));
        }
    }

    /// <summary>
    /// Snapshot of every request the mock received, in order.
    /// </summary>
    public IReadOnlyList<RecordedRequest> RecordedRequests => _server.LogEntries
        .Select(e => new RecordedRequest(
            e.RequestMessage.Method,
            e.RequestMessage.AbsolutePath,
            new Dictionary<string, string>(
                (e.RequestMessage.Headers ?? new Dictionary<string, WireMockList<string>>())
                    .ToDictionary(kv => kv.Key, kv => string.Join(",", kv.Value)),
                StringComparer.OrdinalIgnoreCase),
            e.RequestMessage.Body ?? ""))
        .ToList();

    public async ValueTask DisposeAsync()
    {
        _server.Stop();
        _server.Dispose();
        await Task.CompletedTask;
    }

    internal (int Status, string? Body, Dictionary<string, string> Headers, int DelayMs) Respond((string Method, string Path) key, IList<ScriptEntrySpec> entries)
    {
        int hit;
        lock (_lock)
        {
            var k = $"{key.Method} {key.Path}";
            hit = _hits.GetValueOrDefault(k, 0) + 1;
            _hits[k] = hit;
        }

        // First, exact match_count match. Then, fallthrough (no match_count).
        var entry = entries.FirstOrDefault(e => e.MatchCount == hit)
            ?? entries.FirstOrDefault(e => e.MatchCount is null);

        if (entry is null)
        {
            return (404, $"{{\"error\":\"no rule for hit {hit} on {key.Method} {key.Path}\"}}", new(), 0);
        }

        var body = entry.Respond.Body is null
            ? null
            : JsonSerializer.Serialize(entry.Respond.Body, _jsonOptions);

        return (entry.Respond.Status, body, entry.Respond.Headers, entry.Respond.DelayMs ?? 0);
    }

    /// <summary>
    /// Custom WireMock response provider that delegates to our sequencing
    /// callback. Required because <c>WithCallback</c> on <c>Response.Create</c>
    /// doesn't expose <c>StatusCode</c> dynamically across all WireMock.Net
    /// versions; an explicit provider sidesteps that.
    /// </summary>
    private sealed class SequentialResponseProvider : IResponseProvider
    {
        private readonly MockServerHarness _owner;
        private readonly (string Method, string Path) _key;
        private readonly IList<ScriptEntrySpec> _entries;

        public SequentialResponseProvider(MockServerHarness owner, (string Method, string Path) key, IList<ScriptEntrySpec> entries)
        {
            _owner = owner;
            _key = key;
            _entries = entries;
        }

        public async Task<(WireMock.IResponseMessage Message, WireMock.IMapping? Mapping)> ProvideResponseAsync(
            WireMock.IMapping mapping,
            WireMock.IRequestMessage requestMessage,
            WireMockServerSettings settings)
        {
            var (status, body, headers, delayMs) = _owner.Respond(_key, _entries);

            if (delayMs > 0)
            {
                await Task.Delay(delayMs).ConfigureAwait(false);
            }

            var response = new WireMock.ResponseMessage
            {
                StatusCode = status,
            };

            if (body is not null)
            {
                response.BodyData = new BodyData
                {
                    DetectedBodyType = BodyType.String,
                    BodyAsString = body,
                };
                response.AddHeader("Content-Type", "application/json");
            }

            foreach (var h in headers)
            {
                response.AddHeader(h.Key, h.Value);
            }

            return (response, mapping);
        }
    }
}

public sealed record RecordedRequest(
    string Method,
    string Path,
    IReadOnlyDictionary<string, string> Headers,
    string Body);
