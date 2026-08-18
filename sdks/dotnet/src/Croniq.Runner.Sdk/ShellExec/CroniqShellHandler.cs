using System.Diagnostics;
using System.Runtime.InteropServices;

using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.ShellExec;

/// <summary>
/// Generic shell-exec handler. Decodes <c>__runner_exec</c> metadata and
/// spawns a subprocess; stdout/stderr are piped into the streaming log
/// writer. Exit code 0 means success; any other code throws and is reported
/// as a failed execution.
///
/// Registered via <c>AddCroniqShellHandler(...)</c> — preferably scoped to
/// the job keys the application intends to run through a shell, or as the
/// catch-all default via the parameterless overload.
///
/// Fail-closed behaviour: a payload that sets the <c>user</c> directive
/// fails the execution (.NET cannot switch the subprocess user), and
/// payload-supplied <c>env</c> names that can hijack process resolution or
/// library loading are rejected unless
/// <see cref="CroniqShellHandlerOptions.AllowUnsafeEnvironment"/> is set.
/// </summary>
public sealed class CroniqShellHandler(
    ILogger<CroniqShellHandler> logger,
    IOptions<CroniqShellHandlerOptions> options) : ICroniqJobHandler
{
    public async Task HandleAsync(CroniqExecutionContext context, CancellationToken cancellationToken)
    {
        if (!CroniqShellDecoder.TryDecode(context.Metadata, out var exec, out var error))
        {
            throw new CroniqHandlerException($"shell-exec metadata missing or invalid: {error}");
        }

        var psi = BuildStartInfo(exec, options.Value);
        logger.LogDebug(
            "spawning subprocess for {JobKey}: {File} {Args}",
            context.JobKey,
            psi.FileName,
            psi.ArgumentList.Count > 0 ? string.Join(' ', psi.ArgumentList) : psi.Arguments);

        using var process = new Process { StartInfo = psi, EnableRaisingEvents = true };

        process.Start();

        await using var stdoutWriter = context.LogWriter.AsLineWriter(LogLevel.Information);
        await using var stderrWriter = context.LogWriter.AsLineWriter(LogLevel.Warning);
        var stdoutPump = process.StandardOutput.BaseStream.CopyToAsync(
            new StreamWriterAdapter(stdoutWriter).BaseStream, cancellationToken);
        var stderrPump = process.StandardError.BaseStream.CopyToAsync(
            new StreamWriterAdapter(stderrWriter).BaseStream, cancellationToken);

        try
        {
            await process.WaitForExitAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            TerminateProcess(process);
            throw;
        }
        finally
        {
            try
            {
                await stdoutPump.ConfigureAwait(false);
            }
            catch { /* swallow during shutdown */ }
            try
            {
                await stderrPump.ConfigureAwait(false);
            }
            catch { /* swallow during shutdown */ }
            await context.LogWriter.FlushAsync(CancellationToken.None).ConfigureAwait(false);
        }

        if (process.ExitCode != 0)
        {
            throw new CroniqHandlerException($"subprocess exited with code {process.ExitCode}");
        }
    }

    internal static ProcessStartInfo BuildStartInfo(RunnerExec exec, CroniqShellHandlerOptions options) =>
        BuildStartInfo(exec, options, RuntimeInformation.IsOSPlatform(OSPlatform.Windows));

    /// <summary>
    /// Pure <see cref="ProcessStartInfo"/> construction, factored out (with an
    /// explicit <paramref name="isWindows"/> switch) so unit tests can pin the
    /// quoting behaviour of both platform branches without spawning anything.
    /// </summary>
    internal static ProcessStartInfo BuildStartInfo(RunnerExec exec, CroniqShellHandlerOptions options, bool isWindows)
    {
        ProcessStartInfo psi;
        switch (exec)
        {
            case RunnerExec.Shell sh:
                {
                    RejectUserDirective(sh.User);
                    if (isWindows)
                    {
                        // Deliberate raw pass-through: `cmd.exe /c` parses the
                        // remainder of its command line itself, so the whole
                        // string is one cmd command line — exactly like the
                        // POSIX branch hands one string to `sh -c`. Routing the
                        // command through ArgumentList would layer Win32
                        // argv-quoting on top of cmd's own parsing and corrupt
                        // commands containing quotes.
                        psi = new ProcessStartInfo("cmd.exe") { Arguments = "/c " + sh.Command };
                    }
                    else
                    {
                        // ArgumentList hands the command to sh as a single argv
                        // entry — no quoting/escaping round-trip. Mirrors the
                        // Rust shell runner's `Command::new("sh").arg("-c").arg(cmd)`.
                        psi = new ProcessStartInfo("/bin/sh");
                        psi.ArgumentList.Add("-c");
                        psi.ArgumentList.Add(sh.Command);
                    }
                    if (!string.IsNullOrEmpty(sh.Workdir))
                    {
                        psi.WorkingDirectory = sh.Workdir;
                    }
                    ApplyEnv(psi, sh.Env, options);
                    break;
                }
            case RunnerExec.Exec ex:
                {
                    if (ex.Argv.Count == 0)
                    {
                        throw new CroniqHandlerException("exec.argv is empty");
                    }
                    RejectUserDirective(ex.User);
                    psi = new ProcessStartInfo(ex.Argv[0]);
                    for (var i = 1; i < ex.Argv.Count; i++)
                    {
                        psi.ArgumentList.Add(ex.Argv[i]);
                    }
                    if (!string.IsNullOrEmpty(ex.Workdir))
                    {
                        psi.WorkingDirectory = ex.Workdir;
                    }
                    ApplyEnv(psi, ex.Env, options);
                    break;
                }
            default:
                throw new CroniqHandlerException($"unknown RunnerExec subtype {exec.GetType().Name}");
        }

        psi.RedirectStandardOutput = true;
        psi.RedirectStandardError = true;
        psi.UseShellExecute = false;
        return psi;
    }

    /// <summary>
    /// .NET cannot setuid, so a payload that asks for a different user must
    /// fail the job rather than silently run as the runner's own user (the
    /// Rust shell runner honours numeric uids on unix; see #431 for its
    /// fail-open shape). An empty string counts as "not set", mirroring the
    /// Rust implementation.
    /// </summary>
    private static void RejectUserDirective(string? user)
    {
        if (!string.IsNullOrEmpty(user))
        {
            throw new CroniqHandlerException(
                $"user directive is not supported by the .NET shell handler: cannot run as '{user}'. " +
                "Run the runner process itself as the desired user, or use the Rust croniq-shell-runner, which honours numeric uids.");
        }
    }

    /// <summary>
    /// Env names whose value redirects which binaries or libraries get loaded.
    /// Compared case-insensitively (Windows env names are case-insensitive,
    /// and a conservative guard should not depend on the platform).
    /// </summary>
    private static readonly string[] BlockedEnvNames = ["PATH", "PATHEXT", "COMSPEC", "LD_PRELOAD", "LD_LIBRARY_PATH"];

    /// <summary>Blocked prefixes: dyld injection (macOS) and the SDK's own configuration namespace.</summary>
    private static readonly string[] BlockedEnvPrefixes = ["DYLD_", "CRONIQ_"];

    private static void ApplyEnv(ProcessStartInfo psi, IReadOnlyDictionary<string, string>? env, CroniqShellHandlerOptions options)
    {
        if (env is null)
        {
            return;
        }
        foreach (var kvp in env)
        {
            if (!options.AllowUnsafeEnvironment && IsBlockedEnvName(kvp.Key))
            {
                throw new CroniqHandlerException(
                    $"payload env variable '{kvp.Key}' is blocked: it can hijack process resolution or library loading. " +
                    "Set CroniqShellHandlerOptions.AllowUnsafeEnvironment = true to accept it.");
            }
            psi.Environment[kvp.Key] = kvp.Value;
        }
    }

    private static bool IsBlockedEnvName(string name)
    {
        foreach (var blocked in BlockedEnvNames)
        {
            if (string.Equals(name, blocked, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }
        foreach (var prefix in BlockedEnvPrefixes)
        {
            if (name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }
        return false;
    }

    private static void TerminateProcess(Process process)
    {
        if (process.HasExited)
        {
            return;
        }
        try
        {
            process.Kill(entireProcessTree: true);
        }
        catch
        {
            // best effort
        }
    }

    /// <summary>Tiny TextWriter→Stream bridge so CopyToAsync works against ILogWriter.AsLineWriter().</summary>
    private sealed class StreamWriterAdapter : IDisposable
    {
        public Stream BaseStream { get; }

        public StreamWriterAdapter(TextWriter inner)
        {
            BaseStream = new TextWriterStream(inner);
        }

        public void Dispose() => BaseStream.Dispose();

        private sealed class TextWriterStream(TextWriter inner) : Stream
        {
            public override bool CanRead => false;
            public override bool CanSeek => false;
            public override bool CanWrite => true;
            public override long Length => throw new NotSupportedException();
            public override long Position
            {
                get => throw new NotSupportedException();
                set => throw new NotSupportedException();
            }
            public override void Flush() => inner.Flush();
            public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
            public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
            public override void SetLength(long value) => throw new NotSupportedException();
            public override void Write(byte[] buffer, int offset, int count)
            {
                var text = System.Text.Encoding.UTF8.GetString(buffer, offset, count);
                inner.Write(text);
            }
            public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken)
            {
                Write(buffer, offset, count);
                return Task.CompletedTask;
            }
        }
    }
}
