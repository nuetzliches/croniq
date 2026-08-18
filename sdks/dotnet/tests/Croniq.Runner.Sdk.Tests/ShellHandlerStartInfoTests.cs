using Croniq.Runner.Sdk.ShellExec;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Pins the <c>ProcessStartInfo</c> construction of the shell handler
/// (issue #442): POSIX commands travel as a single argv entry with no
/// quoting round-trip, Windows keeps the deliberate raw pass-through to
/// <c>cmd.exe /c</c>, the <c>user</c> directive fails closed, and
/// payload-supplied env names that hijack process/library resolution are
/// rejected unless explicitly allowed.
/// </summary>
public class ShellHandlerStartInfoTests
{
    private static readonly CroniqShellHandlerOptions Defaults = new();

    [Fact]
    public void PosixShell_CommandWithEmbeddedDoubleQuotes_SurvivesVerbatim()
    {
        // Pre-fix, the handler wrapped the command in "..." and escaped `"`
        // but not `\`, so .NET's Arguments re-parse corrupted the command.
        var command = "echo \"hello world\" && printf '%s' \"nested \\\" quote\"";

        var psi = CroniqShellHandler.BuildStartInfo(
            new RunnerExec.Shell(command), Defaults, isWindows: false);

        psi.FileName.ShouldBe("/bin/sh");
        psi.ArgumentList.ShouldBe(["-c", command]);
        psi.Arguments.ShouldBe(string.Empty); // no string round-trip at all
    }

    [Fact]
    public void PosixShell_CommandEndingInBackslash_SurvivesVerbatim()
    {
        // Pre-fix, a trailing backslash escaped the synthetic closing quote:
        // `-c "printf %s foo\"` handed sh a different command entirely.
        var command = "printf %s foo\\";

        var psi = CroniqShellHandler.BuildStartInfo(
            new RunnerExec.Shell(command), Defaults, isWindows: false);

        psi.ArgumentList.Count.ShouldBe(2);
        psi.ArgumentList[0].ShouldBe("-c");
        psi.ArgumentList[1].ShouldBe(command);
    }

    [Fact]
    public void WindowsShell_KeepsRawPassThroughToCmd()
    {
        // cmd.exe's /c parses the remainder of the line itself; ArgumentList
        // would layer Win32 argv-quoting on top and corrupt quoted commands.
        // The raw pass-through is deliberate — this test pins it.
        var command = "echo \"hello world\" & dir C:\\";

        var psi = CroniqShellHandler.BuildStartInfo(
            new RunnerExec.Shell(command), Defaults, isWindows: true);

        psi.FileName.ShouldBe("cmd.exe");
        psi.Arguments.ShouldBe("/c " + command);
        psi.ArgumentList.ShouldBeEmpty();
    }

    [Fact]
    public void Shell_AppliesWorkdirEnvAndRedirects()
    {
        var env = new Dictionary<string, string> { ["FOO"] = "bar" };

        var psi = CroniqShellHandler.BuildStartInfo(
            new RunnerExec.Shell("true", Workdir: "/srv/app", Env: env), Defaults, isWindows: false);

        psi.WorkingDirectory.ShouldBe("/srv/app");
        psi.Environment["FOO"].ShouldBe("bar");
        psi.RedirectStandardOutput.ShouldBeTrue();
        psi.RedirectStandardError.ShouldBeTrue();
        psi.UseShellExecute.ShouldBeFalse();
    }

    [Fact]
    public void Shell_UserDirective_FailsClosed()
    {
        var ex = Should.Throw<CroniqHandlerException>(() =>
            CroniqShellHandler.BuildStartInfo(
                new RunnerExec.Shell("true", User: "deploy"), Defaults, isWindows: false));

        ex.Message.ShouldContain("user directive is not supported by the .NET shell handler");
    }

    [Fact]
    public void Exec_UserDirective_FailsClosed()
    {
        var ex = Should.Throw<CroniqHandlerException>(() =>
            CroniqShellHandler.BuildStartInfo(
                new RunnerExec.Exec(["/bin/true"], User: "0"), Defaults, isWindows: false));

        ex.Message.ShouldContain("user directive is not supported by the .NET shell handler");
    }

    [Fact]
    public void Shell_EmptyUser_IsTreatedAsUnset()
    {
        // Mirrors the Rust shell runner, which ignores an empty `user`.
        Should.NotThrow(() =>
            CroniqShellHandler.BuildStartInfo(
                new RunnerExec.Shell("true", User: string.Empty), Defaults, isWindows: false));
    }

    [Theory]
    [InlineData("PATH")]
    [InlineData("Path")] // case-insensitive
    [InlineData("PATHEXT")]
    [InlineData("COMSPEC")]
    [InlineData("LD_PRELOAD")]
    [InlineData("ld_preload")] // case-insensitive
    [InlineData("LD_LIBRARY_PATH")]
    [InlineData("DYLD_INSERT_LIBRARIES")]
    [InlineData("CRONIQ_SERVER_URL")]
    public void Env_BlockedNames_FailTheJobByDefault(string name)
    {
        var env = new Dictionary<string, string> { [name] = "hijack" };

        var ex = Should.Throw<CroniqHandlerException>(() =>
            CroniqShellHandler.BuildStartInfo(
                new RunnerExec.Shell("true", Env: env), Defaults, isWindows: false));

        ex.Message.ShouldContain(name);
        ex.Message.ShouldContain("AllowUnsafeEnvironment");
    }

    [Fact]
    public void Env_BlockedNames_ApplyToExecFormToo()
    {
        var env = new Dictionary<string, string> { ["LD_PRELOAD"] = "/tmp/evil.so" };

        Should.Throw<CroniqHandlerException>(() =>
            CroniqShellHandler.BuildStartInfo(
                new RunnerExec.Exec(["/bin/true"], Env: env), Defaults, isWindows: false));
    }

    [Fact]
    public void Env_AllowUnsafeEnvironment_AcceptsBlockedNames()
    {
        var env = new Dictionary<string, string> { ["LD_PRELOAD"] = "/opt/trace.so" };
        var options = new CroniqShellHandlerOptions { AllowUnsafeEnvironment = true };

        var psi = CroniqShellHandler.BuildStartInfo(
            new RunnerExec.Shell("true", Env: env), options, isWindows: false);

        psi.Environment["LD_PRELOAD"].ShouldBe("/opt/trace.so");
    }

    [Fact]
    public void Env_BenignNames_PassThrough()
    {
        var env = new Dictionary<string, string>
        {
            ["APP_ENV"] = "production",
            ["MY_CRONIQ_FLAG"] = "ok", // CRONIQ_ is a prefix match, not a substring match
        };

        var psi = CroniqShellHandler.BuildStartInfo(
            new RunnerExec.Shell("true", Env: env), Defaults, isWindows: false);

        psi.Environment["APP_ENV"].ShouldBe("production");
        psi.Environment["MY_CRONIQ_FLAG"].ShouldBe("ok");
    }
}
