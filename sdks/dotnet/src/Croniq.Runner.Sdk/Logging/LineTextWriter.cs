using System.Globalization;
using System.Text;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.Logging;

/// <summary>
/// A <see cref="TextWriter"/> that splits its incoming character stream on
/// line breaks and forwards one <see cref="WorkEvent"/> per line to the
/// underlying <see cref="ILogWriter"/>. Useful for piping subprocess
/// stdout/stderr directly into a Croniq execution's log stream.
/// </summary>
internal sealed class LineTextWriter(ILogWriter writer, LogLevel level) : TextWriter
{
    private readonly StringBuilder _buffer = new();
    private readonly object _lock = new();
    private bool _disposed;

    public override Encoding Encoding => Encoding.UTF8;

    public override IFormatProvider FormatProvider => CultureInfo.InvariantCulture;

    public override void Write(char value)
    {
        lock (_lock)
        {
            _buffer.Append(value);
        }
        if (value == '\n')
        {
            FlushPendingLines();
        }
    }

    public override void Write(string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return;
        }
        lock (_lock)
        {
            _buffer.Append(value);
        }
        if (value.Contains('\n', StringComparison.Ordinal))
        {
            FlushPendingLines();
        }
    }

    public override void WriteLine(string? value)
    {
        Write(value);
        Write('\n');
    }

    public override void Flush()
    {
        FlushPendingLines(flushTrailingPartial: true);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing && !_disposed)
        {
            _disposed = true;
            FlushPendingLines(flushTrailingPartial: true);
        }
        base.Dispose(disposing);
    }

    private void FlushPendingLines(bool flushTrailingPartial = false)
    {
        List<string>? lines = null;
        lock (_lock)
        {
            while (true)
            {
                var newline = _buffer.IndexOf('\n');
                if (newline < 0)
                {
                    if (flushTrailingPartial && _buffer.Length > 0)
                    {
                        lines ??= [];
                        lines.Add(_buffer.ToString());
                        _buffer.Clear();
                    }
                    break;
                }

                var line = _buffer.ToString(0, newline);
                if (line.Length > 0 && line[^1] == '\r')
                {
                    line = line[..^1];
                }
                _buffer.Remove(0, newline + 1);
                lines ??= [];
                lines.Add(line);
            }
        }

        if (lines is null)
        {
            return;
        }

        foreach (var line in lines)
        {
            // Fire-and-forget: the underlying writer's channel handles backpressure;
            // an unbounded sync wait here would defeat the streaming design.
            // Convert ValueTask to Task so the discard analyzer (CA2012) is satisfied.
            _ = writer.WriteAsync(level, line, fields: null, cancellationToken: default).AsTask();
        }
    }
}

internal static class StringBuilderExtensions
{
    public static int IndexOf(this StringBuilder sb, char value)
    {
        for (var i = 0; i < sb.Length; i++)
        {
            if (sb[i] == value)
            {
                return i;
            }
        }
        return -1;
    }
}
