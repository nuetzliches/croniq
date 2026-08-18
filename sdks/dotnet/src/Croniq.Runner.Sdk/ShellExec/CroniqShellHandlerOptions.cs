namespace Croniq.Runner.Sdk.ShellExec;

/// <summary>
/// Options for <see cref="CroniqShellHandler"/>. Configure via the
/// <c>AddCroniqShellHandler(...)</c> overloads that take an
/// <see cref="Action{T}"/>, or via
/// <c>services.Configure&lt;CroniqShellHandlerOptions&gt;(...)</c>.
/// </summary>
public sealed class CroniqShellHandlerOptions
{
    /// <summary>
    /// When <c>false</c> (the default), payload-supplied <c>env</c> names
    /// that can hijack process resolution or library loading —
    /// <c>PATH</c>, <c>PATHEXT</c>, <c>COMSPEC</c>, <c>LD_PRELOAD</c>,
    /// <c>LD_LIBRARY_PATH</c>, anything starting with <c>DYLD_</c> — and
    /// anything starting with <c>CRONIQ_</c> (the SDK's own configuration
    /// namespace) fail the execution instead of being applied. Names are
    /// compared case-insensitively. Set to <c>true</c> only if the runner
    /// fully trusts the server to supply arbitrary environment variables.
    /// </summary>
    public bool AllowUnsafeEnvironment { get; set; }
}
