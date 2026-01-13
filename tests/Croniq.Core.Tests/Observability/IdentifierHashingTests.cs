using System;
using System.Security.Cryptography;
using System.Text;
using Croniq.Core.Observability;
using Croniq.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Observability;

[CollectionDefinition("IdentifierHashing", DisableParallelization = true)]
public sealed class IdentifierHashingCollection
{
}

[Collection("IdentifierHashing")]
public sealed class IdentifierHashingTests : IDisposable
{
    [Fact]
    public void HashTenantId_returns_input_when_disabled()
    {
        IdentifierHashing.Configure(new CroniqObservabilityOptions { HashIdentifiers = false });

        IdentifierHashing.HashTenantId("tenant-a").ShouldBe("tenant-a");
    }

    [Fact]
    public void HashTenantId_hashes_when_enabled()
    {
        var key = "croniq-test-key";
        IdentifierHashing.Configure(new CroniqObservabilityOptions
        {
            HashIdentifiers = true,
            IdentifierHashKey = key
        });

        var hashed = IdentifierHashing.HashTenantId("tenant-a");
        hashed.ShouldBe(ComputeHmac("tenant-a", key));
    }

    [Fact]
    public void Configure_throws_when_enabled_without_key()
    {
        Should.Throw<InvalidOperationException>(() =>
            IdentifierHashing.Configure(new CroniqObservabilityOptions { HashIdentifiers = true }));
    }

    public void Dispose()
    {
        IdentifierHashing.Configure(new CroniqObservabilityOptions { HashIdentifiers = false });
    }

    private static string ComputeHmac(string value, string key)
    {
        using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(key));
        var hash = hmac.ComputeHash(Encoding.UTF8.GetBytes(value));
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}
