using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.SqlServer;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class PasswordAuthEndpointsTests : IClassFixture<PasswordAuthApiTestHost>
{
    private readonly PasswordAuthApiTestHost _host;

    public PasswordAuthEndpointsTests(PasswordAuthApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task Login_issues_access_token_that_authenticates_me()
    {
        // The password user store generates the UserId.
        var username = "alice";
        var password = "correct horse battery staple";

        var auth = _host.Services.GetRequiredService<PasswordAuthService>();
        var hash = auth.HashPassword("usr_test", username, password);

        var user = await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var login = new PasswordLoginRequest(
            PasswordAuthApiTestHost.TenantId,
            username,
            password,
            PasswordAuthApiTestHost.Environment,
            Scopes: new[] { CroniqScopes.TenantsAdmin },
            Audience: null);

        var loginResponse = await _host.Client.PostAsJsonAsync("/auth/login", login);
        loginResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var body = await loginResponse.Content.ReadFromJsonAsync<TokenEnvelope>();
        body.ShouldNotBeNull();
        body!.AccessToken.ShouldNotBeNullOrWhiteSpace();
        body.RefreshToken.ShouldNotBeNullOrWhiteSpace();

        _host.Client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", body.AccessToken);

        var meResponse = await _host.Client.GetAsync("/me");
        meResponse.StatusCode.ShouldBe(HttpStatusCode.OK);

        var me = await meResponse.Content.ReadFromJsonAsync<CallerInfoEnvelope>();
        me.ShouldNotBeNull();
        me!.TenantId.ShouldBe(PasswordAuthApiTestHost.TenantId);
        me.CallerId.ShouldBe(user.UserId);
        me.CallerType.ShouldBe((int)CallerType.User);
    }

    [Fact]
    public async Task Refresh_rotates_refresh_token_and_old_one_stops_working()
    {
        var userId = "usr_refresh";
        var username = "bob";
        var password = "p@ssw0rd";

        var auth = _host.Services.GetRequiredService<PasswordAuthService>();
        var hash = auth.HashPassword(userId, username, password);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var loginResponse = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            PasswordAuthApiTestHost.TenantId,
            username,
            password,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        var login = await loginResponse.Content.ReadFromJsonAsync<TokenEnvelope>();
        loginResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        login.ShouldNotBeNull();

        var firstRefresh = login!.RefreshToken;
        firstRefresh.ShouldNotBeNullOrWhiteSpace();

        var refreshResponse = await _host.Client.PostAsJsonAsync("/auth/refresh", new PasswordRefreshRequest(
            PasswordAuthApiTestHost.TenantId,
            firstRefresh!,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        refreshResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var refreshed = await refreshResponse.Content.ReadFromJsonAsync<TokenEnvelope>();
        refreshed.ShouldNotBeNull();
        refreshed!.RefreshToken.ShouldNotBe(firstRefresh);

        var secondAttempt = await _host.Client.PostAsJsonAsync("/auth/refresh", new PasswordRefreshRequest(
            PasswordAuthApiTestHost.TenantId,
            firstRefresh!,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        secondAttempt.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);
    }

    [Fact]
    public async Task Login_locks_out_after_too_many_failures()
    {
        var userId = "usr_lock";
        var username = "carol";
        var password = "right";

        var auth = _host.Services.GetRequiredService<PasswordAuthService>();
        var hash = auth.HashPassword(userId, username, password);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var first = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            PasswordAuthApiTestHost.TenantId,
            username,
            "wrong",
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        first.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);

        var second = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            PasswordAuthApiTestHost.TenantId,
            username,
            "wrong",
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        second.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    private sealed class TokenEnvelope
    {
        public string? AccessToken { get; set; }
        public string? TokenType { get; set; }
        public int? ExpiresIn { get; set; }
        public string? RefreshToken { get; set; }
    }

    private sealed class CallerInfoEnvelope
    {
        public string? TenantId { get; set; }
        public string? EnvironmentTag { get; set; }
        public int CallerType { get; set; }
        public string? CallerId { get; set; }
        public string[]? Scopes { get; set; }
    }
}
