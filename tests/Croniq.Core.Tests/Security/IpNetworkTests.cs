using System.Net;
using Croniq.Core.Security;
using FluentAssertions;
using Xunit;

namespace Croniq.Core.Tests.Security;

public class IpNetworkTests
{
    [Fact]
    public void ParseAndContainsReturnsExpectedForIpv4()
    {
        var result = IpNetwork.TryParse("10.20.0.0/24", out var network, out var error);

        result.Should().BeTrue(error);
        network.Should().NotBeNull();
        network!.Contains(IPAddress.Parse("10.20.0.5")).Should().BeTrue();
        network.Contains(IPAddress.Parse("10.21.0.1")).Should().BeFalse();
    }

    [Fact]
    public void AllowsIpv4MappedIpv6Addresses()
    {
        IpNetwork.TryParse("192.168.1.0/24", out var network, out var error).Should().BeTrue(error);
        network.Should().NotBeNull();

        var mappedAddress = IPAddress.Parse("192.168.1.25").MapToIPv6();
        network!.Contains(mappedAddress).Should().BeTrue();
    }

    [Fact]
    public void RejectsInvalidCidrs()
    {
        IpNetwork.TryParse("10.0.0.0", out _, out var missingPrefix).Should().BeFalse();
        missingPrefix.Should().Be("cidr-format");

        IpNetwork.TryParse("10.0.0.0/99", out _, out var rangeError).Should().BeFalse();
        rangeError.Should().Be("cidr-prefix-range");

        IpNetwork.TryParse("invalid/24", out _, out var addressError).Should().BeFalse();
        addressError.Should().Be("cidr-address");
    }
}
