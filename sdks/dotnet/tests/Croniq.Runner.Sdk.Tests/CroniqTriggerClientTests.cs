using System.Net;
using System.Text;
using System.Text.Json;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Internal;

using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Wire-level coverage for <see cref="CroniqTriggerClient"/>: request shape
/// (snake_case, null omission), response parsing (including the
/// forward-compatible <c>deduplicated</c> flag), and error propagation.
/// </summary>
public class CroniqTriggerClientTests
{
    [Fact]
    public async Task PostsSnakeCaseBodyToTriggerEndpoint()
    {
        var stub = new StubHandler(HttpStatusCode.OK, """{"execution_id":"exec-1","queued":3}""");
        var client = CreateClient(stub);

        var result = await client.TriggerAsync(
            "billing:invoice-generate",
            metadata: new Dictionary<string, string> { ["invoice_id"] = "inv_42" },
            require: ["billing"],
            prefer: ["eu-central"],
            timeout: "10m",
            idempotencyKey: "evt-123");

        stub.LastRequest!.RequestUri!.AbsolutePath.ShouldBe("/v1/trigger");
        stub.LastRequest.Method.ShouldBe(HttpMethod.Post);

        using var body = JsonDocument.Parse(stub.LastRequestBody!);
        var root = body.RootElement;
        root.GetProperty("job_key").GetString().ShouldBe("billing:invoice-generate");
        root.GetProperty("metadata").GetProperty("invoice_id").GetString().ShouldBe("inv_42");
        root.GetProperty("require")[0].GetString().ShouldBe("billing");
        root.GetProperty("prefer")[0].GetString().ShouldBe("eu-central");
        root.GetProperty("timeout").GetString().ShouldBe("10m");
        root.GetProperty("idempotency_key").GetString().ShouldBe("evt-123");

        result.ExecutionId.ShouldBe("exec-1");
        result.Queued.ShouldBe(3);
    }

    [Fact]
    public async Task OmitsUnsetOptionalFields()
    {
        var stub = new StubHandler(HttpStatusCode.OK, """{"execution_id":"exec-1","queued":1}""");
        var client = CreateClient(stub);

        await client.TriggerAsync("etl:data-sync");

        using var body = JsonDocument.Parse(stub.LastRequestBody!);
        var root = body.RootElement;
        root.GetProperty("job_key").GetString().ShouldBe("etl:data-sync");
        root.TryGetProperty("metadata", out _).ShouldBeFalse();
        root.TryGetProperty("require", out _).ShouldBeFalse();
        root.TryGetProperty("prefer", out _).ShouldBeFalse();
        root.TryGetProperty("timeout", out _).ShouldBeFalse();
        root.TryGetProperty("idempotency_key", out _).ShouldBeFalse();
    }

    [Fact]
    public async Task MissingDeduplicatedFlagDefaultsToFalse()
    {
        // Older servers don't send `deduplicated` at all.
        var stub = new StubHandler(HttpStatusCode.OK, """{"execution_id":"exec-1","queued":0}""");
        var client = CreateClient(stub);

        var result = await client.TriggerAsync("etl:data-sync");

        result.Deduplicated.ShouldBeFalse();
    }

    [Fact]
    public async Task DeduplicatedFlagIsSurfaced()
    {
        var stub = new StubHandler(
            HttpStatusCode.OK,
            """{"execution_id":"exec-1","queued":0,"deduplicated":true}""");
        var client = CreateClient(stub);

        var result = await client.TriggerAsync("etl:data-sync", idempotencyKey: "evt-1");

        result.Deduplicated.ShouldBeTrue();
        result.ExecutionId.ShouldBe("exec-1");
    }

    [Fact]
    public async Task NonSuccessStatusThrows()
    {
        var stub = new StubHandler(HttpStatusCode.NotFound, """{"error":"unknown job"}""");
        var client = CreateClient(stub);

        await Should.ThrowAsync<HttpRequestException>(
            () => client.TriggerAsync("nope:missing"));
    }

    [Fact]
    public async Task BlankJobKeyThrows()
    {
        var stub = new StubHandler(HttpStatusCode.OK, """{"execution_id":"x","queued":0}""");
        var client = CreateClient(stub);

        await Should.ThrowAsync<ArgumentException>(() => client.TriggerAsync("  "));
    }

    private static CroniqTriggerClient CreateClient(StubHandler stub)
    {
        var http = new HttpClient(stub)
        {
            BaseAddress = new Uri("http://example.test:4000"),
            Timeout = Timeout.InfiniteTimeSpan,
        };
        var monitor = new StubOptionsMonitor(new CroniqClientOptions
        {
            ServerUrl = "http://example.test:4000",
            ApiKey = "croniq_trigger_key",
        });
        return new CroniqTriggerClient(http, monitor);
    }

    private sealed class StubHandler(HttpStatusCode status, string responseBody) : HttpMessageHandler
    {
        public HttpRequestMessage? LastRequest { get; private set; }
        public string? LastRequestBody { get; private set; }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            LastRequest = request;
            LastRequestBody = request.Content is null
                ? null
                : await request.Content.ReadAsStringAsync(cancellationToken);
            return new HttpResponseMessage(status)
            {
                Content = new StringContent(responseBody, Encoding.UTF8, "application/json"),
            };
        }
    }

    private sealed class StubOptionsMonitor(CroniqClientOptions value) : IOptionsMonitor<CroniqClientOptions>
    {
        public CroniqClientOptions CurrentValue { get; } = value;
        public CroniqClientOptions Get(string? name) => CurrentValue;
        public IDisposable? OnChange(Action<CroniqClientOptions, string?> listener) => null;
    }
}
