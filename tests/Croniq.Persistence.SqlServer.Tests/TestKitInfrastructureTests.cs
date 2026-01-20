using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Croniq.TestKit.Diagnostics;
using Croniq.TestKit.Infrastructure;
using DotNet.Testcontainers.Containers;
using NSubstitute;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

public class TestKitInfrastructureTests
{
    [Fact]
    public void RepositoryLocator_ResolvesSolutionRoot()
    {
        var root = RepositoryLocator.Root;

        File.Exists(Path.Combine(root, "croniq.slnx")).ShouldBeTrue();
    }

    [Fact]
    public void GetArtifactsDirectory_CreatesFolder()
    {
        var path = RepositoryLocator.GetArtifactsDirectory(Path.Combine("tests", "test-kit"));

        Directory.Exists(path).ShouldBeTrue();
    }

    [Fact]
    public async Task TestcontainerLogCollector_WritesLogs()
    {
        var container = Substitute.For<ITestcontainersContainer>();
        var path = await TestcontainerLogCollector.CaptureContainerLogsAsync(
            container,
            "My:Container*Name",
            CancellationToken.None);

        var fileName = Path.GetFileNameWithoutExtension(path);
        fileName.ShouldStartWith("my-container-name-");
        File.ReadAllText(path).ShouldContain("Container log capture is not available");

        File.Delete(path);
    }
}
