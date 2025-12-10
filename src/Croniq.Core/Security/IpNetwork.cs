using System;
using System.Globalization;
using System.Net;
using System.Net.Sockets;

namespace Croniq.Core.Security;

public sealed class IpNetwork
{
    private IpNetwork(IPAddress networkAddress, int prefixLength)
    {
        NetworkAddress = networkAddress;
        PrefixLength = prefixLength;
    }

    public IPAddress NetworkAddress { get; }

    public int PrefixLength { get; }

    public AddressFamily AddressFamily => NetworkAddress.AddressFamily;

    public static bool TryParse(string? value, out IpNetwork? network, out string? error)
    {
        network = null;
        error = null;

        if (string.IsNullOrWhiteSpace(value))
        {
            error = "cidr-empty";
            return false;
        }

        var parts = value.Split('/', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length != 2)
        {
            error = "cidr-format";
            return false;
        }

        if (!IPAddress.TryParse(parts[0], out var address))
        {
            error = "cidr-address";
            return false;
        }

        if (!int.TryParse(parts[1], NumberStyles.Integer, CultureInfo.InvariantCulture, out var prefixLength))
        {
            error = "cidr-prefix";
            return false;
        }

        var maxPrefix = address.AddressFamily == AddressFamily.InterNetwork ? 32 : 128;
        if (prefixLength < 0 || prefixLength > maxPrefix)
        {
            error = "cidr-prefix-range";
            return false;
        }

        network = new IpNetwork(Normalize(address, prefixLength), prefixLength);
        return true;
    }

    public bool Contains(IPAddress address)
    {
        if (!TryAlignAddress(address, NetworkAddress.AddressFamily, out var candidate))
        {
            return false;
        }

        var networkBytes = NetworkAddress.GetAddressBytes();
        var addressBytes = candidate.GetAddressBytes();
        var fullBytes = PrefixLength / 8;
        var remainingBits = PrefixLength % 8;

        for (var i = 0; i < fullBytes; i++)
        {
            if (addressBytes[i] != networkBytes[i])
            {
                return false;
            }
        }

        if (remainingBits == 0)
        {
            return true;
        }

        var mask = (byte)~(0xFF >> remainingBits);
        return (addressBytes[fullBytes] & mask) == (networkBytes[fullBytes] & mask);
    }

    public override string ToString()
    {
        return $"{NetworkAddress}/{PrefixLength}";
    }

    private static IPAddress Normalize(IPAddress address, int prefixLength)
    {
        var bytes = address.GetAddressBytes();
        var fullBytes = prefixLength / 8;
        var remainingBits = prefixLength % 8;

        if (fullBytes < bytes.Length)
        {
            if (remainingBits > 0)
            {
                var mask = (byte)~(0xFF >> remainingBits);
                bytes[fullBytes] &= mask;
                fullBytes++;
            }

            for (var i = fullBytes; i < bytes.Length; i++)
            {
                bytes[i] = 0;
            }
        }
        else if (prefixLength == 0)
        {
            Array.Clear(bytes, 0, bytes.Length);
        }

        return new IPAddress(bytes);
    }

    private static bool TryAlignAddress(IPAddress address, AddressFamily targetFamily, out IPAddress aligned)
    {
        if (address.AddressFamily == targetFamily)
        {
            aligned = address;
            return true;
        }

        if (targetFamily == AddressFamily.InterNetwork
            && address.AddressFamily == AddressFamily.InterNetworkV6
            && address.IsIPv4MappedToIPv6)
        {
            aligned = address.MapToIPv4();
            return true;
        }

        aligned = address;
        return false;
    }
}
