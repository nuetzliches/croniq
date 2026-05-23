using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.ShellExec;

/// <summary>
/// Decoded form of the <c>__runner_exec</c> metadata produced by the
/// Croniqfile compiler for <c>runner shell { ... }</c> and
/// <c>runner exec { ... }</c> blocks. Discriminator: <c>kind</c>.
/// </summary>
[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(typeof(Shell), typeDiscriminator: "shell")]
[JsonDerivedType(typeof(Exec), typeDiscriminator: "exec")]
public abstract record RunnerExec
{
    /// <summary>Shell-style: a single command string interpreted by <c>/bin/sh -c</c> (or platform equivalent).</summary>
    public sealed record Shell(
        [property: JsonPropertyName("command")] string Command,
        [property: JsonPropertyName("workdir")] string? Workdir = null,
        [property: JsonPropertyName("user")] string? User = null,
        [property: JsonPropertyName("env")] IReadOnlyDictionary<string, string>? Env = null)
        : RunnerExec;

    /// <summary>Exec-style: argv array, no shell interpolation.</summary>
    public sealed record Exec(
        [property: JsonPropertyName("argv")] IReadOnlyList<string> Argv,
        [property: JsonPropertyName("workdir")] string? Workdir = null,
        [property: JsonPropertyName("user")] string? User = null,
        [property: JsonPropertyName("env")] IReadOnlyDictionary<string, string>? Env = null)
        : RunnerExec;
}
