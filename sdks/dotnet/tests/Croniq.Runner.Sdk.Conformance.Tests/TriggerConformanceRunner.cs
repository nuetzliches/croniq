using System.Text.Json;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;

using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Orchestrator for a single <b>trigger (producer)</b> conformance case: starts
/// the scripted mock server, wires the SDK's DI with the case's credentials,
/// makes each scripted <c>TriggerAsync(...)</c> call in order asserting its
/// expected outcome, then asserts the recorded request stream against
/// <c>expectations.http</c>.
/// </summary>
/// <remarks>
/// Unlike <see cref="ConformanceRunner"/> there is no poll loop to wait on: a
/// producer case makes explicit calls and is done, so expectations are asserted
/// once after the last call rather than polled until satisfied.
///
/// Deliberately goes through <see cref="CroniqClientServiceCollectionExtensions.AddCroniqClient(IServiceCollection, Action{CroniqClientOptions}?)"/>
/// rather than newing up the client: the <c>Authorization</c> header is applied
/// by a DI-registered <c>CroniqClientAuthHandler</c>, so constructing the
/// client directly would leave the auth expectations in cases like
/// <c>06-trigger-auth-apikey</c> asserting a header no real caller path
/// produced.
/// </remarks>
internal static class TriggerConformanceRunner
{
    public static async Task RunAsync(TriggerCaseSpec spec)
    {
        await using var mock = new MockServerHarness(spec.ServerScript);
        await using var services = BuildServices(spec, mock.BaseUrl);
        var client = services.GetRequiredService<ICroniqTriggerClient>();

        foreach (var call in spec.TriggerCalls)
        {
            await InvokeAndAssertAsync(client, call);
        }

        AssertExpectations(spec.Expectations, mock.RecordedRequests);
    }

    private static ServiceProvider BuildServices(TriggerCaseSpec spec, string baseUrl)
    {
        var services = new ServiceCollection();
        services.AddCroniqClient(o =>
        {
            o.ServerUrl = baseUrl;
            if (spec.TriggerConfig.ApiKey is not null)
            {
                o.ApiKey = spec.TriggerConfig.ApiKey;
            }
            if (spec.TriggerConfig.BearerToken is not null)
            {
                o.BearerToken = spec.TriggerConfig.BearerToken;
            }
        });
        return services.BuildServiceProvider();
    }

    private static async Task InvokeAndAssertAsync(ICroniqTriggerClient client, TriggerCallSpec call)
    {
        var r = call.Request;
        var expectError = call.Expect.Error == true;

        TriggerResult? result = null;
        Exception? thrown = null;
        try
        {
            result = await client.TriggerAsync(
                r.JobKey,
                metadata: r.Metadata,
                require: r.Require,
                prefer: r.Prefer,
                timeout: r.Timeout,
                idempotencyKey: r.IdempotencyKey);
        }
        catch (Exception ex)
        {
            thrown = ex;
        }

        if (expectError)
        {
            Assert.True(
                thrown is not null,
                $"trigger({r.JobKey}): expected an error but got result {result}");
            return;
        }

        Assert.True(
            thrown is null,
            $"trigger({r.JobKey}): expected a response but the client threw {thrown}");

        AssertResponse(r.JobKey, call.Expect.Response, result!);
    }

    private static void AssertResponse(string jobKey, TriggerExpectedResponseSpec? expected, TriggerResult actual)
    {
        if (expected is null)
        {
            return;
        }

        if (expected.ExecutionId is { } wantId)
        {
            if (wantId == "*")
            {
                Assert.False(
                    string.IsNullOrEmpty(actual.ExecutionId),
                    $"trigger({jobKey}): expected non-empty execution_id (*) but was '{actual.ExecutionId}'");
            }
            else
            {
                Assert.True(
                    wantId == actual.ExecutionId,
                    $"trigger({jobKey}): expected execution_id '{wantId}' but got '{actual.ExecutionId}'");
            }
        }

        if (expected.Queued is { } wantQueued)
        {
            Assert.True(
                wantQueued == actual.Queued,
                $"trigger({jobKey}): expected queued={wantQueued} but got {actual.Queued}");
        }

        if (expected.Deduplicated is { } wantDedup)
        {
            Assert.True(
                wantDedup == actual.Deduplicated,
                $"trigger({jobKey}): expected deduplicated={wantDedup} but got {actual.Deduplicated}");
        }
    }

    private static void AssertExpectations(
        TriggerExpectationsSpec expectations,
        IReadOnlyList<RecordedRequest> recorded)
    {
        foreach (var e in expectations.Http)
        {
            var matches = recorded
                .Where(r => string.Equals(r.Method, e.Method, StringComparison.OrdinalIgnoreCase)
                            && r.Path == e.Path)
                .ToList();

            if (e.ExactCount is { } exact)
            {
                Assert.True(
                    matches.Count == exact,
                    $"Expected {e.Method} {e.Path} exact_count={exact}, got {matches.Count}");
            }
            if (e.MinCount is { } min)
            {
                Assert.True(
                    matches.Count >= min,
                    $"Expected {e.Method} {e.Path} min_count={min}, got {matches.Count}");
            }
            if (e.MaxCount is { } max)
            {
                Assert.True(
                    matches.Count <= max,
                    $"Expected {e.Method} {e.Path} max_count={max}, got {matches.Count}");
            }

            AssertHeaders(e, matches);
            AssertBody(e, matches);
        }
    }

    private static void AssertHeaders(TriggerHttpExpectation e, List<RecordedRequest> matches)
    {
        if (e.Headers.Count == 0)
        {
            return;
        }
        Assert.True(
            matches.Count > 0,
            $"Headers expected on {e.Method} {e.Path} but no requests recorded");

        var first = matches[0];
        foreach (var h in e.Headers)
        {
            Assert.True(
                first.Headers.TryGetValue(h.Key, out var actual),
                $"Missing header '{h.Key}' on {e.Method} {e.Path}. Headers seen: {string.Join(", ", first.Headers.Keys)}");

            if (h.Value == "*")
            {
                Assert.False(
                    string.IsNullOrEmpty(actual),
                    $"Header '{h.Key}' expected non-empty (*) but was empty");
            }
            else
            {
                Assert.True(
                    h.Value == actual,
                    $"Header '{h.Key}' expected '{h.Value}' but was '{actual}'");
            }
        }
    }

    private static void AssertBody(TriggerHttpExpectation e, List<RecordedRequest> matches)
    {
        if (e.BodyMatch is null && e.BodyAbsent.Count == 0)
        {
            return;
        }
        Assert.True(
            matches.Count > 0,
            $"body expectation on {e.Method} {e.Path} but no requests recorded");

        var first = matches[0];
        using var doc = JsonDocument.Parse(string.IsNullOrEmpty(first.Body) ? "null" : first.Body);
        var root = doc.RootElement;

        if (e.BodyMatch is not null)
        {
            var err = BodyMatcher.Match(e.BodyMatch, root);
            Assert.True(
                err is null,
                $"body_match failed on {e.Method} {e.Path}: {err}. Actual: {first.Body}");
        }

        foreach (var key in e.BodyAbsent)
        {
            Assert.False(
                root.ValueKind == JsonValueKind.Object && root.TryGetProperty(key, out _),
                $"body_absent violated on {e.Method} {e.Path}: key '{key}' present. Actual: {first.Body}");
        }
    }
}
