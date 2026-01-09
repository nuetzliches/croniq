using System.IO;
using System.Threading;
using Croniq.Api;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class GrpcDisconnectsTests
{
    [Fact]
    public void IsExpected_ReturnsTrue_ForRequestStreamReset()
    {
        var exception = new IOException("The client reset the request stream.");

        GrpcDisconnects.IsExpected(exception, CancellationToken.None)
            .ShouldBeTrue();
    }

    [Fact]
    public void IsExpected_ReturnsFalse_ForGenericIOException()
    {
        var exception = new IOException("disk read failure");

        GrpcDisconnects.IsExpected(exception, CancellationToken.None)
            .ShouldBeFalse();
    }
}
