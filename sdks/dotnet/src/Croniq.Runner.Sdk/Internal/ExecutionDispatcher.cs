using System.Diagnostics;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.Logging;
using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging;

#pragma warning disable CA1031 // generic catch in handler boundary is intentional

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Owns the lifecycle of a single in-flight execution: builds the
/// <see cref="CroniqExecutionContext"/>, runs the handler under a per-execution
/// linked CancellationToken, periodically renews the work-item lease,
/// drains the streaming log writer (if used), and acks the outcome.
/// </summary>
internal sealed class ExecutionDispatcher(
    ICroniqClient client,
    CroniqHandlerRegistry registry,
    IServiceProvider serviceProvider,
    ILoggerFactory loggerFactory,
    RunnerStateProbe stateProbe,
    CroniqRunnerOptions options,
    string runnerId,
    IReadOnlyList<string> runnerTags,
    TimeProvider timeProvider)
{
    private readonly ILogger _logger = loggerFactory.CreateLogger<ExecutionDispatcher>();

    public Task DispatchAsync(
        WorkAssignment assignment,
        CancellationTokenSource executionCts,
        CancellationToken outerCt)
    {
        stateProbe.IncrementInflight();
        // Run on the thread pool so the poll loop can immediately accept more work.
        return Task.Run(() => RunOneAsync(assignment, executionCts, outerCt), CancellationToken.None);
    }

    private async Task RunOneAsync(
        WorkAssignment assignment,
        CancellationTokenSource executionCts,
        CancellationToken outerCt)
    {
        var executionId = assignment.ExecutionId;
        var jobKey = assignment.JobKey;
        var attempt = assignment.Attempt;
        var executionTimeout = ParseTimeout(assignment.Timeout) ?? TimeSpan.FromMinutes(5);
        var handlerLogger = loggerFactory.CreateLogger($"CroniqJob.{jobKey}");

        using var scopedLogger = handlerLogger.BeginScope(new Dictionary<string, object>
        {
            ["execution_id"] = executionId,
            ["job_key"] = jobKey,
            ["runner_id"] = runnerId,
            ["attempt"] = attempt,
        });

        var enrichment = new LogEnrichment(jobKey, runnerId, runnerTags);
        var writerLogger = loggerFactory.CreateLogger<LogWriter>();
        var lazyWriter = new Lazy<ILogWriter>(
            () => new LogWriter(client, executionId, enrichment, options.LogWriter, writerLogger, timeProvider),
            LazyThreadSafetyMode.ExecutionAndPublication);

        Task pushEventInline(WorkEvent ev, CancellationToken ct) =>
            client.PushEventsAsync(executionId, [enrichment.Enrich(ev)], ct);

        var ctx = new CroniqExecutionContext(
            executionId, jobKey, attempt, assignment.Metadata, executionTimeout,
            runnerId, runnerTags, executionCts.Token, handlerLogger, lazyWriter, pushEventInline);

        using var activity = CroniqInstrumentation.ActivitySource.StartActivity(
            $"croniq.execute {jobKey}",
            ActivityKind.Consumer);
        activity?.SetTag(CroniqAttributes.JobKey, jobKey);
        activity?.SetTag(CroniqAttributes.ExecutionId, executionId);
        activity?.SetTag(CroniqAttributes.ExecutionAttempt, attempt);
        activity?.SetTag(CroniqAttributes.RunnerId, runnerId);
        if (runnerTags.Count > 0)
        {
            activity?.SetTag(CroniqAttributes.RunnerTags, string.Join(',', runnerTags));
        }
        activity?.SetTag(CroniqAttributes.ExecutionTimeout, executionTimeout.ToString());

        var metricsTags = new TagList
        {
            { "job_key", jobKey },
            { "runner_id", runnerId },
        };
        CroniqInstrumentation.ExecutionsInflight.Add(1, metricsTags);

        var stopwatch = Stopwatch.StartNew();
        string status;
        string? error = null;

        // Lease renewal loop runs alongside the handler
        using var renewCts = CancellationTokenSource.CreateLinkedTokenSource(executionCts.Token);
        var renewTask = RenewLoopAsync(executionId, renewCts.Token);

        try
        {
            if (!registry.TryGet(jobKey, out var entry))
            {
                throw new NoHandlerRegisteredException(jobKey);
            }
            await entry.InvokeAsync(serviceProvider, ctx, executionCts.Token).ConfigureAwait(false);
            status = "success";
        }
        catch (OperationCanceledException) when (executionCts.IsCancellationRequested && outerCt.IsCancellationRequested)
        {
            status = "failure";
            error = "runner draining";
        }
        catch (OperationCanceledException) when (executionCts.IsCancellationRequested)
        {
            status = "failure";
            error = "cancelled by server";
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "handler for {JobKey} (execution {ExecutionId}) threw", jobKey, executionId);
            status = "failure";
            error = ex.Message;
        }
        finally
        {
            stopwatch.Stop();
            await renewCts.CancelAsync().ConfigureAwait(false);
            try
            {
                await renewTask.ConfigureAwait(false);
            }
            catch
            {
                // expected on cancellation
            }
            CroniqInstrumentation.ExecutionsInflight.Add(-1, metricsTags);
        }

        var outcomeTags = new TagList
        {
            { "job_key", jobKey },
            { "runner_id", runnerId },
            { "outcome", status },
        };
        CroniqInstrumentation.ExecutionsCompleted.Add(1, outcomeTags);
        CroniqInstrumentation.ExecutionDuration.Record(stopwatch.Elapsed.TotalMilliseconds, outcomeTags);
        if (status == "failure")
        {
            CroniqInstrumentation.ExecutionsFailed.Add(1, outcomeTags);
        }
        activity?.SetTag(CroniqAttributes.Outcome, status);
        if (status == "failure")
        {
            activity?.SetStatus(ActivityStatusCode.Error, error);
        }
        else
        {
            activity?.SetStatus(ActivityStatusCode.Ok);
        }

        if (lazyWriter.IsValueCreated)
        {
            try
            {
                await lazyWriter.Value.DisposeAsync().ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                _logger.LogWarning(ex, "log_writer drain failed for execution {ExecutionId}", executionId);
            }
        }

        try
        {
            await client.AckAsync(
                new AckRequest(runnerId, executionId, status, error, stopwatch.ElapsedMilliseconds, attempt),
                CancellationToken.None).ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _logger.LogError(ex, "failed to ack execution {ExecutionId}", executionId);
        }

        stateProbe.DecrementInflight();
    }

    private async Task RenewLoopAsync(string executionId, CancellationToken ct)
    {
        try
        {
            while (!ct.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(options.RenewInterval, ct).ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    return;
                }

                try
                {
                    await client.RenewAsync(new RenewRequest(runnerId, executionId), ct).ConfigureAwait(false);
                }
                catch (Exception ex) when (ex is not OperationCanceledException)
                {
                    _logger.LogDebug(ex, "lease renew failed for execution {ExecutionId}", executionId);
                }
            }
        }
        catch (OperationCanceledException)
        {
            // shutdown
        }
    }

    private static TimeSpan? ParseTimeout(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
        {
            return null;
        }
        // Accept duration strings like "15m", "30s", "1h" — match the Croniqfile parser.
        // For ISO-8601 durations we'd use XmlConvert, but the server emits humane forms.
        var span = raw.Trim().ToLowerInvariant();
        if (span.Length < 2)
        {
            return null;
        }
        var unit = span[^1];
        if (!double.TryParse(span[..^1], System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var value))
        {
            return null;
        }
        return unit switch
        {
            's' => TimeSpan.FromSeconds(value),
            'm' => TimeSpan.FromMinutes(value),
            'h' => TimeSpan.FromHours(value),
            'd' => TimeSpan.FromDays(value),
            _ => null,
        };
    }
}
