using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Execution;

/// <summary>
/// Forwards execution log entries to the regular ILogger pipeline so configured sinks (e.g., OTLP) receive them.
/// </summary>
public sealed class LoggerExecutionLogExporter : IExecutionLogExporter
{
    private readonly ILogger<LoggerExecutionLogExporter> _logger;

    public LoggerExecutionLogExporter(ILogger<LoggerExecutionLogExporter> logger)
    {
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
    }

    public Task ExportAsync(IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken)
    {
        foreach (var entry in entries)
        {
            if (cancellationToken.IsCancellationRequested)
            {
                break;
            }

            using var scope = _logger.BeginScope(new Dictionary<string, object?>
            {
                { "croniq.execution_id", entry.ExecutionId },
                { "croniq.correlation_id", entry.CorrelationId },
                { "croniq.trace_id", entry.TraceId },
                { "croniq.span_id", entry.SpanId }
            });

            _logger.Log(
                entry.Level,
                default,
                entry.Properties,
                null,
                (props, _) => entry.RenderedMessage ?? entry.MessageTemplate);
        }

        return Task.CompletedTask;
    }
}
