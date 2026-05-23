using System.Text.Json;

using Croniq.Runner.Sdk.ShellExec;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

public class ShellDecoderTests
{
    [Fact]
    public void TryDecode_ParsesShellPayload()
    {
        var metadata = JsonDocument.Parse(
            """{ "__runner_exec": "{\"kind\":\"shell\",\"command\":\"echo hi\"}" }""").RootElement;

        var ok = CroniqShellDecoder.TryDecode(metadata, out var exec, out var error);

        ok.ShouldBeTrue(error);
        exec.ShouldBeOfType<RunnerExec.Shell>();
        ((RunnerExec.Shell)exec!).Command.ShouldBe("echo hi");
    }

    [Fact]
    public void TryDecode_ParsesExecPayload()
    {
        var metadata = JsonDocument.Parse(
            """{ "__runner_exec": "{\"kind\":\"exec\",\"argv\":[\"/bin/ls\",\"-la\"]}" }""").RootElement;

        var ok = CroniqShellDecoder.TryDecode(metadata, out var exec, out var error);

        ok.ShouldBeTrue(error);
        exec.ShouldBeOfType<RunnerExec.Exec>();
        ((RunnerExec.Exec)exec!).Argv.ShouldBe(["/bin/ls", "-la"]);
    }

    [Fact]
    public void TryDecode_ReturnsFalseWhenKeyMissing()
    {
        var metadata = JsonDocument.Parse("{}").RootElement;

        var ok = CroniqShellDecoder.TryDecode(metadata, out var exec, out var error);

        ok.ShouldBeFalse();
        exec.ShouldBeNull();
        error.ShouldNotBeNullOrEmpty();
    }
}
