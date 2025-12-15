using System.Reflection;
using System.Security.Claims;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class CallerContextFactoryUnitTests
{
    private static readonly IOptionsMonitor<CroniqOidcOptions> DisabledOidc =
        new StubOptionsMonitor<CroniqOidcOptions>(new CroniqOidcOptions { Enabled = false });

    [Fact]
    public async Task FromApiKeyAsync_returns_null_for_empty_input()
    {
        var factory = new CallerContextFactory(new InMemoryApiKeyStore(), DisabledOidc, NullLogger<CallerContextFactory>.Instance);

        (await factory.FromApiKeyAsync(" ")).ShouldBeNull();
    }

    [Fact]
    public async Task FromApiKeyAsync_returns_null_when_validation_fails()
    {
        var factory = new CallerContextFactory(new InMemoryApiKeyStore(), DisabledOidc, NullLogger<CallerContextFactory>.Instance);

        (await factory.FromApiKeyAsync("ak_x.invalid")).ShouldBeNull();
    }

    [Theory]
    [InlineData(null, null)]
    [InlineData("", null)]
    [InlineData("   ", null)]
    [InlineData("Bearer abc", "abc")]
    [InlineData("bearer   abc  ", "abc")]
    [InlineData("abc", "abc")]
    public void ExtractBearerToken_parses_expected_values(string? input, string? expected)
    {
        var token = InvokePrivateStatic<string?>(typeof(CallerContextFactory), "ExtractBearerToken", new object?[] { input });
        token.ShouldBe(expected);
    }

    [Fact]
    public async Task FromBearerTokenAsync_returns_null_when_disabled_or_authority_missing()
    {
        var store = new InMemoryApiKeyStore();

        var disabled = new CallerContextFactory(store, DisabledOidc, NullLogger<CallerContextFactory>.Instance);
        (await disabled.FromBearerTokenAsync("Bearer whatever")).ShouldBeNull();

        var enabledNoAuthority = new CallerContextFactory(
            store,
            new StubOptionsMonitor<CroniqOidcOptions>(new CroniqOidcOptions { Enabled = true, Authority = "" }),
            NullLogger<CallerContextFactory>.Instance);
        (await enabledNoAuthority.FromBearerTokenAsync("Bearer whatever")).ShouldBeNull();
    }

    [Fact]
    public void FindFirst_uses_primary_then_fallbacks_ignoring_empty_values()
    {
        var principal = new ClaimsPrincipal(new ClaimsIdentity(new[]
        {
            new Claim("primary", ""),
            new Claim("fb1", "tenant-x")
        }));

        var value = InvokePrivateStatic<string?>(typeof(CallerContextFactory), "FindFirst",
            principal,
            "primary",
            (IReadOnlyCollection<string>)new[] { "fb1", "fb2" });

        value.ShouldBe("tenant-x");
    }

    [Fact]
    public void ResolveEnvironment_prefers_claim_then_falls_back_to_default()
    {
        var principal = new ClaimsPrincipal(new ClaimsIdentity(new[]
        {
            new Claim("env", "prod")
        }));

        var options = new CroniqOidcOptions
        {
            EnvironmentClaim = "env",
            DefaultEnvironment = "dev"
        };

        var resolved = InvokePrivateStatic<string?>(typeof(CallerContextFactory), "ResolveEnvironment", principal, options);
        resolved.ShouldBe("prod");

        var noClaimPrincipal = new ClaimsPrincipal(new ClaimsIdentity());
        var resolvedFallback = InvokePrivateStatic<string?>(typeof(CallerContextFactory), "ResolveEnvironment", noClaimPrincipal, options);
        resolvedFallback.ShouldBe("dev");
    }

    [Fact]
    public void ResolveCallerId_uses_claim_then_identity_name_then_default()
    {
        var withClaim = new ClaimsPrincipal(new ClaimsIdentity(new[] { new Claim("sub", "user-1") }));
        var options = new CroniqOidcOptions { CallerIdClaim = "sub" };
        InvokePrivateStatic<string>(typeof(CallerContextFactory), "ResolveCallerId", withClaim, options).ShouldBe("user-1");

        var withName = new ClaimsPrincipal(new ClaimsIdentity(new[] { new Claim(ClaimTypes.Name, "bob") }));
        InvokePrivateStatic<string>(typeof(CallerContextFactory), "ResolveCallerId", withName, options).ShouldBe("bob");

        var empty = new ClaimsPrincipal(new ClaimsIdentity());
        InvokePrivateStatic<string>(typeof(CallerContextFactory), "ResolveCallerId", empty, options).ShouldBe("oidc-user");
    }

    [Fact]
    public void ResolveScopes_splits_dedupes_and_can_normalize_to_lowercase()
    {
        var principal = new ClaimsPrincipal(new ClaimsIdentity(new[]
        {
            new Claim("scope", "A B"),
            new Claim("scp", "b"),
            new Claim("scope", "  ")
        }));

        var options = new CroniqOidcOptions { NormalizeScopesToLowercase = true };
        var scopes = InvokePrivateStatic<IReadOnlyCollection<string>>(typeof(CallerContextFactory), "ResolveScopes", principal, options);

        scopes.ShouldContain("a");
        scopes.ShouldContain("b");
        scopes.Count.ShouldBe(2);
    }

    [Fact]
    public void HasAllScopes_is_case_insensitive_and_requires_all()
    {
        InvokePrivateStatic<bool>(typeof(CallerContextFactory), "HasAllScopes",
                (IReadOnlyCollection<string>)new[] { "jobs:read", "schedules:write" },
                (IReadOnlyCollection<string>)new[] { "JOBS:READ" })
            .ShouldBeTrue();

        InvokePrivateStatic<bool>(typeof(CallerContextFactory), "HasAllScopes",
                (IReadOnlyCollection<string>)new[] { "jobs:read" },
                (IReadOnlyCollection<string>)new[] { "jobs:read", "schedules:write" })
            .ShouldBeFalse();
    }

    private static T InvokePrivateStatic<T>(Type type, string methodName, params object?[] args)
    {
        var method = type.GetMethod(methodName, BindingFlags.NonPublic | BindingFlags.Static);
        method.ShouldNotBeNull($"Missing method {type.FullName}.{methodName}");
        return (T)method!.Invoke(null, args)!;
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
