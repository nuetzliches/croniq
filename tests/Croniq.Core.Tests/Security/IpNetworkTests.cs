using System.Net;
using Croniq.Core.Security;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Security;

public class IpNetworkTests
{
    [Fact]
    public void ParseAndContainsReturnsExpectedForIpv4()
    {
        var result = IpNetwork.TryParse("10.20.0.0/24", out var network, out var error);

        result.ShouldBeTrue(error);
        network.ShouldNotBeNull();
        network!.Contains(IPAddress.Parse("10.20.0.5")).ShouldBeTrue();
        network.Contains(IPAddress.Parse("10.21.0.1")).ShouldBeFalse();
    }

    [Fact]
    public void AllowsIpv4MappedIpv6Addresses()
    {
        IpNetwork.TryParse("192.168.1.0/24", out var network, out var error).ShouldBeTrue(error);
        network.ShouldNotBeNull();

        var mappedAddress = IPAddress.Parse("192.168.1.25").MapToIPv6();
        network!.Contains(mappedAddress).ShouldBeTrue();
    }

    [Fact]
    public void RejectsInvalidCidrs()
    {
        IpNetwork.TryParse("10.0.0.0", out _, out var missingPrefix).ShouldBeFalse();
        missingPrefix.ShouldBe("cidr-format");

        IpNetwork.TryParse("10.0.0.0/99", out _, out var rangeError).ShouldBeFalse();
        rangeError.ShouldBe("cidr-prefix-range");

        IpNetwork.TryParse("invalid/24", out _, out var addressError).ShouldBeFalse();
        addressError.ShouldBe("cidr-address");
    }
}
