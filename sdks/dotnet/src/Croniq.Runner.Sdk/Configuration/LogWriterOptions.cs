namespace Croniq.Runner.Sdk.Configuration;

/// <summary>
/// Tunables for the streaming <c>ILogWriter</c>. Defaults mirror the Rust
/// runner SDK constants so behaviour is consistent across implementations.
/// </summary>
public sealed class LogWriterOptions
{
    /// <summary>Bounded channel capacity. Backpressure kicks in when full.</summary>
    public int ChannelCapacity { get; set; } = 256;

    /// <summary>Flush when this many events have accumulated.</summary>
    public int BatchSizeThreshold { get; set; } = 32;

    /// <summary>Flush at least this often, even if <see cref="BatchSizeThreshold"/> isn't reached.</summary>
    public TimeSpan BatchTimeThreshold { get; set; } = TimeSpan.FromMilliseconds(200);

    /// <summary>Maximum events per outgoing HTTP POST.</summary>
    public int MaxBatchPerPost { get; set; } = 100;

    /// <summary>
    /// Maximum time the runner waits for queued events to flush during
    /// per-execution drain before sending the ack.
    /// </summary>
    public TimeSpan ShutdownTimeout { get; set; } = TimeSpan.FromSeconds(5);
}
