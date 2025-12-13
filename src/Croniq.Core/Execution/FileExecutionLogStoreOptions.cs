namespace Croniq.Core.Execution;

/// <summary>
/// Options for the filesystem-based execution log store.
/// </summary>
public sealed class FileExecutionLogStoreOptions
{
    /// <summary>
    /// Base directory where execution logs are written. Default: "logs".
    /// </summary>
    public string BasePath { get; set; } = "logs";

    /// <summary>
    /// Optional shard length (characters from ExecutionId) to reduce directory fan-out. Default: 2.
    /// </summary>
    public int ShardPrefixLength { get; set; } = 2;
}
