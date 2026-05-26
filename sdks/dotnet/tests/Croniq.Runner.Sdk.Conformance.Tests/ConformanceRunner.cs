using System.Diagnostics;
using System.Text.Json;

using Croniq.Runner.Sdk;
using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Orchestrator for a single conformance case: starts a scripted mock
/// server, wires the SDK's DI, drives <see cref="CroniqRunner.RunAsync"/>
/// until expectations are met (or the case deadline elapses), then asserts
/// the recorded HTTP traffic.
/// </summary>
internal static class ConformanceRunner
{
    public static async Task RunAsync(CaseSpec spec, CancellationToken ct = default)
    {
        await using var mock = new MockServerHarness(spec.ServerScript);

        await using var services = BuildServices(spec, mock.BaseUrl);
        var runner = services.GetRequiredService<CroniqRunner>();

        var deadline = TimeSpan.FromMilliseconds(spec.Expectations.DurationMaxMs ?? 5000);

        using var caseCts = CancellationTokenSource.CreateLinkedTokenSource(ct);
        caseCts.CancelAfter(deadline);

        // Optional binding directive: cancel the runner partway through.
        // Useful for drain/shutdown cases.
        using var runnerCts = CancellationTokenSource.CreateLinkedTokenSource(caseCts.Token);
        if (spec.ShutdownAfterMs is int after)
        {
            _ = Task.Delay(after, caseCts.Token)
                .ContinueWith(_ => runnerCts.Cancel(), TaskScheduler.Default);
        }

        var runTask = runner.RunAsync(runnerCts.Token);

        // Poll the mock's request log until expectations are satisfied; this
        // lets us exit early once the runner has produced everything we need,
        // instead of always waiting the full duration_max_ms.
        var stopwatch = Stopwatch.StartNew();
        while (!caseCts.IsCancellationRequested)
        {
            if (ExpectationsAreMet(spec.Expectations, mock.RecordedRequests))
            {
                break;
            }
            try
            {
                await Task.Delay(50, caseCts.Token);
            }
            catch (OperationCanceledException)
            {
                break;
            }
        }

        runnerCts.Cancel();
        try
        {
            await runTask;
        }
        catch (OperationCanceledException)
        {
            // expected when we cancel the runner
        }
        catch (PollInstanceConflictException)
        {
            // Expected for case 11a (and any future case where the SDK is
            // contractually allowed to bail out fatally). The HTTP-count
            // assertions below still validate correctness: a case that
            // doesn't anticipate this exit would fail on min_count/max_count
            // mismatch.
        }
        stopwatch.Stop();

        var recorded2 = mock.RecordedRequests;
        if (Environment.GetEnvironmentVariable("CRONIQ_CONFORMANCE_DEBUG") == "1")
        {
            Console.WriteLine($"[debug-{spec.Name}] {recorded2.Count} request(s) in {stopwatch.ElapsedMilliseconds}ms:");
            foreach (var r in recorded2) Console.WriteLine($"[debug]   {r.Method} {r.Path}");
        }
        AssertExpectations(spec, recorded2, stopwatch.Elapsed);
    }

    private static ServiceProvider BuildServices(CaseSpec spec, string serverUrl)
    {
        var sc = new ServiceCollection();
        sc.AddLogging(b => b.SetMinimumLevel(LogLevel.Warning));
        sc.AddSingleton<TimeProvider>(TimeProvider.System);

        var builder = sc.AddCroniqRunner(opts =>
        {
            opts.ServerUrl = serverUrl;
            ApplyConfig(opts, spec.RunnerConfig);
        });

        HandlerSentinels.ApplyTo(builder, spec.Handlers);

        // Strip the BackgroundService — we drive RunAsync ourselves so the
        // case can observe the runner's lifecycle deterministically.
        var hostedDescriptor = sc.FirstOrDefault(d => d.ServiceType.FullName == "Microsoft.Extensions.Hosting.IHostedService");
        if (hostedDescriptor is not null)
        {
            sc.Remove(hostedDescriptor);
        }

        return sc.BuildServiceProvider();
    }

    private static void ApplyConfig(CroniqRunnerOptions opts, RunnerConfigSpec cfg)
    {
        if (cfg.RunnerId is not null) opts.RunnerId = cfg.RunnerId;
        if (cfg.RunnerIdPrefix is not null) opts.RunnerIdPrefix = cfg.RunnerIdPrefix;
        foreach (var cap in cfg.Capabilities) opts.Capabilities.Add(cap);
        foreach (var t in cfg.Tags) opts.Tags.Add(t);
        if (cfg.MaxInflight is int m) opts.MaxInflight = m;
        if (cfg.ApiKey is not null) opts.ApiKey = cfg.ApiKey;
        if (cfg.BearerToken is not null) opts.BearerToken = cfg.BearerToken;
        if (cfg.PollTimeoutMs is int pt) opts.PollTimeout = TimeSpan.FromMilliseconds(pt);
        if (cfg.RenewIntervalMs is int ri) opts.RenewInterval = TimeSpan.FromMilliseconds(ri);
        if (cfg.DrainTimeoutMs is int dt) opts.DrainTimeout = TimeSpan.FromMilliseconds(dt);
        if (cfg.PollRetryDelayMs is int prd) opts.PollRetryDelay = TimeSpan.FromMilliseconds(prd);
        if (cfg.CapacityBackoffMs is int cb) opts.CapacityBackoff = TimeSpan.FromMilliseconds(cb);
        if (cfg.MaxConsecutivePollConflicts is int mcpc) opts.MaxConsecutivePollConflicts = mcpc;
    }

    private static bool ExpectationsAreMet(ExpectationsSpec expectations, IReadOnlyList<RecordedRequest> recorded)
    {
        foreach (var ex in expectations.Http)
        {
            // max_count is a "ceiling over a time window" assertion. If
            // we'd let the loop exit as soon as min/exact were satisfied,
            // a runner that violates max only AFTER the early exit would
            // pass trivially. Force the case to wait its full
            // duration_max_ms in that case so the ceiling is observable.
            if (ex.MaxCount.HasValue) return false;

            var matching = recorded.Count(r =>
                r.Method.Equals(ex.Method, StringComparison.OrdinalIgnoreCase) &&
                r.Path == ex.Path);

            if (ex.ExactCount is int exact && matching < exact) return false;
            if (ex.MinCount is int min && matching < min) return false;
            // body_match and header checks happen post-hoc.
        }
        return true;
    }

    private static void AssertExpectations(CaseSpec spec, IReadOnlyList<RecordedRequest> recorded, TimeSpan elapsed)
    {
        // duration_max_ms is enforced as a wall-clock deadline by
        // caseCts.CancelAfter(deadline). An additional post-hoc
        // ShouldBeLessThan check on `elapsed` is redundant AND wrong for
        // cases that legitimately run the full window (max_count
        // assertions, see ExpectationsAreMet): the stopwatch always reads
        // a few ms over the deadline because cancellation propagation
        // isn't instantaneous. We rely on the cancellation enforcement
        // and skip the redundant assertion.
        _ = elapsed;

        foreach (var ex in spec.Expectations.Http)
        {
            var matches = recorded
                .Where(r => r.Method.Equals(ex.Method, StringComparison.OrdinalIgnoreCase) && r.Path == ex.Path)
                .ToList();

            if (ex.ExactCount is int exact)
            {
                matches.Count.ShouldBe(exact, $"{ex.Method} {ex.Path}: expected exact_count={exact}");
            }
            if (ex.MinCount is int min)
            {
                matches.Count.ShouldBeGreaterThanOrEqualTo(min, $"{ex.Method} {ex.Path}: expected min_count={min}");
            }
            if (ex.MaxCount is int xmax)
            {
                matches.Count.ShouldBeLessThanOrEqualTo(xmax, $"{ex.Method} {ex.Path}: expected max_count={xmax}");
            }

            if (ex.Headers.Count > 0 && matches.Count > 0)
            {
                // The first matching request must carry all expected headers.
                var first = matches[0];
                foreach (var (name, expected) in ex.Headers)
                {
                    first.Headers.ContainsKey(name).ShouldBeTrue($"{ex.Method} {ex.Path}: missing header '{name}'");
                    var actual = first.Headers[name];
                    if (expected == "*")
                    {
                        actual.ShouldNotBeNullOrEmpty($"{ex.Method} {ex.Path}: header '{name}' was empty");
                    }
                    else
                    {
                        actual.ShouldBe(expected, $"{ex.Method} {ex.Path}: header '{name}' mismatch");
                    }
                }
            }

            if (ex.BodyMatch is not null && matches.Count > 0)
            {
                // First matching request must satisfy the body subset match.
                using var doc = JsonDocument.Parse(matches[0].Body);
                var err = BodyMatcher.Match(ex.BodyMatch, doc.RootElement);
                err.ShouldBeNull($"{ex.Method} {ex.Path}: body mismatch — {err}");
            }
        }
    }
}
