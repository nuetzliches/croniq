using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class AuthCoreInMemoryApiKeyStoreTests
{
    private static readonly IOptionsMonitor<CroniqOidcOptions> DisabledOidc =
        new StubOptionsMonitor<CroniqOidcOptions>(new CroniqOidcOptions { Enabled = false });

    private static readonly IOptionsMonitor<CroniqTokenOptions> DisabledCroniqTokens =
        new StubOptionsMonitor<CroniqTokenOptions>(new CroniqTokenOptions { Enabled = false });

    [Fact]
    public async Task Issue_and_validate_roundtrip_returns_caller_context()
    {
        var store = new InMemoryApiKeyStore();
        var issued = await store.IssueAsync(new ApiKeyIssueRequest("tenant-1", "client-1", "dev", new[] { "schedules:read" }, null));
        var factory = new CallerContextFactory(store, DisabledOidc, DisabledCroniqTokens, NullLogger<CallerContextFactory>.Instance);

        var context = await factory.FromApiKeyAsync(issued.PlaintextSecret);

        context.ShouldNotBeNull();
        context!.TenantId.ShouldBe("tenant-1");
        context.EnvironmentTag.ShouldBe("dev");
        context.CallerType.ShouldBe(CallerType.ApiKey);
        context.CallerId.ShouldBe(issued.KeyId);
        context.Scopes.ShouldContain("schedules:read");
    }

    [Fact]
    public async Task Revoke_marks_key_invalid()
    {
        var store = new InMemoryApiKeyStore();
        var issued = await store.IssueAsync(new ApiKeyIssueRequest("tenant-1", "client-1", null, Array.Empty<string>(), null));

        var revoked = await store.RevokeAsync("tenant-1", issued.KeyId);
        revoked.ShouldBeTrue();

        var validation = await store.ValidateAsync(issued.PlaintextSecret);
        validation.IsValid.ShouldBeFalse();
        validation.Failure.ShouldBe("not-found"); // key is inactive
    }

    [Fact]
    public async Task Rotate_revokes_old_and_emits_new_secret()
    {
        var store = new InMemoryApiKeyStore();
        var issued = await store.IssueAsync(new ApiKeyIssueRequest("tenant-1", "client-1", null, new[] { "x" }, TimeSpan.FromMinutes(30)));

        var rotated = await store.RotateAsync("tenant-1", issued.KeyId);
        rotated.ShouldNotBeNull();

        (await store.ValidateAsync(issued.PlaintextSecret)).IsValid.ShouldBeFalse();
        var newValidation = await store.ValidateAsync(rotated!.PlaintextSecret);
        newValidation.IsValid.ShouldBeTrue();
        newValidation.TenantId.ShouldBe("tenant-1");
        newValidation.CallerId.ShouldBe(rotated.KeyId);
        newValidation.Scopes.ShouldContain("x");
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
