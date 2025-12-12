using System;
using System.Collections.Generic;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Execution;

/// <summary>
/// Represents a persisted log entry emitted during a job execution.
/// </summary>
public sealed record JobLogEntry(
    string ExecutionId,
    DateTimeOffset TimestampUtc,
    LogLevel Level,
    string MessageTemplate,
    string? RenderedMessage,
    string? Exception,
    IReadOnlyDictionary<string, object?> Properties,
    string? TraceId,
    string? SpanId,
    string? CorrelationId,
    long Sequence);
