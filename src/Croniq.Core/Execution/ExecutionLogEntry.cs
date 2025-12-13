using System;
using System.Collections.Generic;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Execution;

/// <summary>
/// Represents a persisted log entry emitted during an execution.
/// </summary>
public sealed record ExecutionLogEntry(
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
