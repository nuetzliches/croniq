using System.Globalization;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using Croniq.Sdk.Operator.Webhooks;
using Shouldly;
using Xunit;

namespace Croniq.Sdk.Tests;

public sealed class WebhookIpRuleClientTests
{
    [Fact]
    public async Task SyncAsync_creates_and_deletes_rules_to_match_desired_set()
    {
        var handler = new FakeWebhookIpRuleHandler([
            new WebhookIpRule(
                1,
                "203.0.113.0/28",
                "corp-egress",
                "seed",
                DateTimeOffset.UtcNow.AddMinutes(-10),
                DateTimeOffset.UtcNow.AddMinutes(-5))
        ]);
        using var httpClient = handler.CreateClient();
        var client = new WebhookIpRuleClient(httpClient);

        var desired = new[]
        {
            new WebhookIpRuleDesired("0.0.0.0/0", "allow-any-v4"),
            new WebhookIpRuleDesired("::/0", "allow-any-v6")
        };

        var result = await client.SyncAsync("tenant-a", "hook_1", "prod", desired);

        result.Created.Count().ShouldBe(2);
        result.DeletedRuleIds.ShouldBe(new[] { 1L });
        result.FinalState
            .Select(rule => rule.Cidr)
            .OrderBy(cidr => cidr, StringComparer.Ordinal)
            .ToArray()
            .ShouldBe(new[] { "0.0.0.0/0", "::/0" }.OrderBy(cidr => cidr, StringComparer.Ordinal).ToArray());
        handler.SeenCorrelationIds
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .Distinct(StringComparer.Ordinal)
            .ShouldHaveSingleItem();
    }

    [Fact]
    public async Task SyncAsync_ignores_duplicate_desired_entries()
    {
        var handler = new FakeWebhookIpRuleHandler(Array.Empty<WebhookIpRule>());
        using var httpClient = handler.CreateClient();
        var client = new WebhookIpRuleClient(httpClient);

        var desired = new[]
        {
            new WebhookIpRuleDesired("0.0.0.0/0", "primary"),
            new WebhookIpRuleDesired("0.0.0.0/0", "duplicate"),
            new WebhookIpRuleDesired(" 0.0.0.0/0 ", "with-whitespace")
        };

        var result = await client.SyncAsync("tenant-a", "hook_1", "prod", desired);

        result.Created.Count().ShouldBe(1);
        result.DeletedRuleIds.ShouldBeEmpty();
        var finalRule = result.FinalState.ShouldHaveSingleItem();
        finalRule.Cidr.ShouldBe("0.0.0.0/0");
    }

    [Fact]
    public async Task SyncAsync_uses_supplied_correlation_id_when_provided()
    {
        var handler = new FakeWebhookIpRuleHandler([
            new WebhookIpRule(
                9,
                "10.0.0.0/24",
                "legacy",
                "seed",
                DateTimeOffset.UtcNow.AddMinutes(-5),
                DateTimeOffset.UtcNow.AddMinutes(-2))
        ]);
        using var httpClient = handler.CreateClient();
        var client = new WebhookIpRuleClient(httpClient);

        var result = await client.SyncAsync(
            "tenant-z",
            "hook_z",
            "prod",
            new[] { new WebhookIpRuleDesired("10.0.0.0/24", "legacy") },
            correlationId: "correlate-123");

        result.Created.ShouldBeEmpty();
        var correlation = handler.SeenCorrelationIds
            .Where(id => !string.IsNullOrWhiteSpace(id))
            .Distinct(StringComparer.Ordinal)
            .ShouldHaveSingleItem();
        correlation.ShouldBe("correlate-123");
    }

    private sealed class FakeWebhookIpRuleHandler : HttpMessageHandler
    {
        private readonly List<WebhookIpRule> _rules;
        private long _nextId;
        private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
        public List<string?> SeenCorrelationIds { get; } = new();

        public FakeWebhookIpRuleHandler(IEnumerable<WebhookIpRule> initialRules)
        {
            _rules = new List<WebhookIpRule>(initialRules ?? Array.Empty<WebhookIpRule>());
            _nextId = _rules.Count == 0 ? 1 : _rules.Max(r => r.Id) + 1;
        }

        public HttpClient CreateClient()
        {
            return new HttpClient(this)
            {
                BaseAddress = new Uri("https://localhost:5001", UriKind.Absolute)
            };
        }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            ArgumentNullException.ThrowIfNull(request);

            if (request.Headers.TryGetValues("X-Croniq-CorrelationId", out var correlationValues))
            {
                SeenCorrelationIds.Add(correlationValues.FirstOrDefault());
            }
            else
            {
                SeenCorrelationIds.Add(null);
            }

            if (request.Method == HttpMethod.Get && request.RequestUri?.AbsolutePath.EndsWith("/ip-rules", StringComparison.OrdinalIgnoreCase) == true)
            {
                return CreateJsonResponse(HttpStatusCode.OK, _rules);
            }

            if (request.Method == HttpMethod.Post && request.RequestUri?.AbsolutePath.EndsWith("/ip-rules", StringComparison.OrdinalIgnoreCase) == true)
            {
                var payload = await request.Content!.ReadFromJsonAsync<WebhookIpRuleCreateRequest>(JsonOptions, cancellationToken);
                var now = DateTimeOffset.UtcNow;
                var rule = new WebhookIpRule(
                    _nextId++,
                    payload!.Cidr,
                    payload.Description,
                    "sdk",
                    now,
                    now);
                _rules.Add(rule);
                return CreateJsonResponse(HttpStatusCode.OK, rule);
            }

            if (request.Method == HttpMethod.Delete && request.RequestUri is not null)
            {
                var idSegment = GetLastPathSegment(request.RequestUri);
                if (!long.TryParse(idSegment, NumberStyles.Integer, CultureInfo.InvariantCulture, out var ruleId))
                {
                    return new HttpResponseMessage(HttpStatusCode.BadRequest);
                }

                var removed = _rules.RemoveAll(rule => rule.Id == ruleId);
                return removed > 0
                    ? new HttpResponseMessage(HttpStatusCode.NoContent)
                    : new HttpResponseMessage(HttpStatusCode.NotFound);
            }

            return new HttpResponseMessage(HttpStatusCode.NotFound);
        }

        private static HttpResponseMessage CreateJsonResponse<T>(HttpStatusCode statusCode, T payload)
        {
            return new HttpResponseMessage(statusCode)
            {
                Content = JsonContent.Create(payload, options: JsonOptions)
            };
        }

        private static string GetLastPathSegment(Uri uri)
        {
            var path = uri.AbsolutePath.TrimEnd('/');
            var index = path.LastIndexOf('/');
            return index >= 0 ? path[(index + 1)..] : path;
        }
    }
}
