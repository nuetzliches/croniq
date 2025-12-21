using System;
using System.Collections.Generic;
using System.Security.Cryptography;
using System.Text;
using Croniq.Api.Security;
using Croniq.Auth.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class TenantRateLimitDeciderTests
{
    [Fact]
    public void GetPartitionId_Hashes_fallback_value()
    {
        var options = new StubOptionsMonitor<CroniqApiOptions>(new CroniqApiOptions());
        var decider = new TenantRateLimitDecider(options);
        var fallback = "ak_test.secret";

        var partition = decider.GetPartitionId(null, fallback);

        partition.ShouldBe("anonymous:" + ComputeHash(fallback));
        partition.ShouldNotContain(fallback);
    }

    [Fact]
    public void GetPartitionId_Uses_tenant_and_caller_when_available()
    {
        var options = new StubOptionsMonitor<CroniqApiOptions>(new CroniqApiOptions());
        var decider = new TenantRateLimitDecider(options);
        var caller = new StubCallerContext("tenant-1", "dev", "client-1");

        var partition = decider.GetPartitionId(caller, "ignored");

        partition.ShouldBe("tenant:tenant-1|caller:client-1");
    }

    private static string ComputeHash(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }

    private sealed record StubCallerContext(string TenantId, string? EnvironmentTag, string CallerId) : ICallerContext
    {
        public CallerType CallerType => CallerType.ApiKey;
        public IReadOnlyCollection<string> Scopes => Array.Empty<string>();
        public bool IsActive => true;
    }

    private sealed class StubOptionsMonitor<T> : IOptionsMonitor<T>
        where T : class, new()
    {
        public StubOptionsMonitor(T currentValue) => CurrentValue = currentValue;
        public T CurrentValue { get; }
        public T Get(string? name) => CurrentValue;
        public IDisposable OnChange(Action<T, string?> listener) => NullDisposable.Instance;

        private sealed class NullDisposable : IDisposable
        {
            public static readonly NullDisposable Instance = new();
            public void Dispose() { }
        }
    }
}
