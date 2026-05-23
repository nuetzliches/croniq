using System.Diagnostics;
using System.Runtime.InteropServices;

using Microsoft.Extensions.Logging;

namespace Croniq.Runner.Sdk.ShellExec;

/// <summary>
/// Generic shell-exec handler. Decodes <c>__runner_exec</c> metadata and
/// spawns a subprocess; stdout/stderr are piped into the streaming log
/// writer. Exit code 0 means success; any other code throws and is reported
/// as a failed execution.
///
/// Registered via <c>AddCroniqShellHandler()</c>, either as a per-job
/// handler or as the catch-all default.
/// </summary>
public sealed class CroniqShellHandler(ILogger<CroniqShellHandler> logger) : ICroniqJobHandler
{
    public async Task HandleAsync(CroniqExecutionContext context, CancellationToken cancellationToken)
    {
        if (!CroniqShellDecoder.TryDecode(context.Metadata, out var exec, out var error))
        {
            throw new CroniqHandlerException($"shell-exec metadata missing or invalid: {error}");
        }

        var psi = BuildStartInfo(exec);
        logger.LogDebug("spawning subprocess for {JobKey}: {File} {Args}", context.JobKey, psi.FileName, psi.Arguments);

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

    private static ProcessStartInfo BuildStartInfo(RunnerExec exec)
    {
        ProcessStartInfo psi;
        switch (exec)
        {
            case RunnerExec.Shell sh:
                {
                    var isWindows = RuntimeInformation.IsOSPlatform(OSPlatform.Windows);
                    psi = isWindows
                        ? new ProcessStartInfo("cmd.exe", $"/c {sh.Command}")
                        : new ProcessStartInfo("/bin/sh", $"-c \"{sh.Command.Replace("\"", "\\\"", StringComparison.Ordinal)}\"");
                    if (!string.IsNullOrEmpty(sh.Workdir))
                    {
                        psi.WorkingDirectory = sh.Workdir;
                    }
                    ApplyEnv(psi, sh.Env);
                    break;
                }
            case RunnerExec.Exec ex:
                {
                    if (ex.Argv.Count == 0)
                    {
                        throw new CroniqHandlerException("exec.argv is empty");
                    }
                    psi = new ProcessStartInfo(ex.Argv[0]);
                    for (var i = 1; i < ex.Argv.Count; i++)
                    {
                        psi.ArgumentList.Add(ex.Argv[i]);
                    }
                    if (!string.IsNullOrEmpty(ex.Workdir))
                    {
                        psi.WorkingDirectory = ex.Workdir;
                    }
                    ApplyEnv(psi, ex.Env);
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

    private static void ApplyEnv(ProcessStartInfo psi, IReadOnlyDictionary<string, string>? env)
    {
        if (env is null)
        {
            return;
        }
        foreach (var kvp in env)
        {
            psi.Environment[kvp.Key] = kvp.Value;
        }
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
