using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
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

        var auth = _host.Services.GetRequiredService<IPasswordAuthService>();
        var hash = auth.HashPassword("usr_test", username, password);

        var user = await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var login = new PasswordLoginRequest(
            username,
            password,
            PasswordAuthApiTestHost.TenantId,
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

        var auth = _host.Services.GetRequiredService<IPasswordAuthService>();
        var hash = auth.HashPassword(userId, username, password);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var loginResponse = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            password,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        var login = await loginResponse.Content.ReadFromJsonAsync<TokenEnvelope>();
        loginResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        login.ShouldNotBeNull();

        var firstRefresh = login!.RefreshToken;
        firstRefresh.ShouldNotBeNullOrWhiteSpace();

        var refreshResponse = await _host.Client.PostAsJsonAsync("/auth/refresh", new PasswordRefreshRequest(
            firstRefresh!,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        refreshResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var refreshed = await refreshResponse.Content.ReadFromJsonAsync<TokenEnvelope>();
        refreshed.ShouldNotBeNull();
        refreshed!.RefreshToken.ShouldNotBe(firstRefresh);

        var secondAttempt = await _host.Client.PostAsJsonAsync("/auth/refresh", new PasswordRefreshRequest(
            firstRefresh!,
            PasswordAuthApiTestHost.TenantId,
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

        var auth = _host.Services.GetRequiredService<IPasswordAuthService>();
        var hash = auth.HashPassword(userId, username, password);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var first = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            "wrong",
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        first.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);

        var second = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            "wrong",
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        second.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
    }

    [Fact]
    public async Task Login_increments_failed_login_count_until_lockout()
    {
        var userId = "usr_failcount";
        var username = "dave";
        var password = "right";

        var auth = _host.Services.GetRequiredService<IPasswordAuthService>();
        var hash = auth.HashPassword(userId, username, password);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var first = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            "wrong",
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        first.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);

        var afterFirst = await _host.Users.FindByUsernameAsync(PasswordAuthApiTestHost.TenantId, username);
        afterFirst.ShouldNotBeNull();
        afterFirst!.FailedLoginCount.ShouldBe(1);
        afterFirst.LockoutEndUtc.ShouldBeNull();

        var second = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            "wrong",
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        second.StatusCode.ShouldBe(HttpStatusCode.Forbidden);

        var afterSecond = await _host.Users.FindByUsernameAsync(PasswordAuthApiTestHost.TenantId, username);
        afterSecond.ShouldNotBeNull();
        afterSecond!.FailedLoginCount.ShouldBe(2);
        afterSecond.LockoutEndUtc.ShouldNotBeNull();
    }

    [Fact]
    public async Task Login_success_clears_failed_login_count_and_lockout()
    {
        var userId = "usr_clears_failcount";
        var username = "frank";
        var password = "right";

        var auth = _host.Services.GetRequiredService<IPasswordAuthService>();
        var hash = auth.HashPassword(userId, username, password);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true));

        var wrong = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            "wrong",
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        wrong.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);

        var afterWrong = await _host.Users.FindByUsernameAsync(PasswordAuthApiTestHost.TenantId, username);
        afterWrong.ShouldNotBeNull();
        afterWrong!.FailedLoginCount.ShouldBe(1);
        afterWrong.LockoutEndUtc.ShouldBeNull();

        var ok = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            password,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        ok.StatusCode.ShouldBe(HttpStatusCode.OK);
        var okBody = await ok.Content.ReadFromJsonAsync<TokenEnvelope>();
        okBody.ShouldNotBeNull();
        okBody!.AccessToken.ShouldNotBeNullOrWhiteSpace();

        var afterOk = await _host.Users.FindByUsernameAsync(PasswordAuthApiTestHost.TenantId, username);
        afterOk.ShouldNotBeNull();
        afterOk!.FailedLoginCount.ShouldBe(0);
        afterOk.LockoutEndUtc.ShouldBeNull();
    }

    [Fact]
    public async Task Change_password_clears_password_change_required_and_old_password_stops_working()
    {
        var userId = "usr_changepw";
        var username = "erin";
        var oldPassword = "old";
        var newPassword = "new";

        var auth = _host.Services.GetRequiredService<IPasswordAuthService>();
        var hash = auth.HashPassword(userId, username, oldPassword);

        await _host.Users.UpsertAsync(new(
            PasswordAuthApiTestHost.TenantId,
            username,
            hash,
            new[] { CroniqScopes.TenantsAdmin },
            IsActive: true,
            PasswordChangeRequired: true));

        var loginResponse = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            oldPassword,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        loginResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var login = await loginResponse.Content.ReadFromJsonAsync<TokenEnvelope>();
        login.ShouldNotBeNull();
        login!.AccessToken.ShouldNotBeNullOrWhiteSpace();
        login.RefreshToken.ShouldNotBeNullOrWhiteSpace();

        _host.Client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", login.AccessToken);

        var changeResponse = await _host.Client.PostAsJsonAsync("/auth/change-password", new
        {
            currentPassword = oldPassword,
            newPassword
        });

        changeResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var refreshAfterChange = await _host.Client.PostAsJsonAsync("/auth/refresh", new PasswordRefreshRequest(
            login.RefreshToken!,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        // Password change revokes all refresh tokens, requiring a new login.
        refreshAfterChange.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);

        var oldLoginAttempt = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            oldPassword,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        oldLoginAttempt.StatusCode.ShouldBe(HttpStatusCode.Unauthorized);

        var newLoginAttempt = await _host.Client.PostAsJsonAsync("/auth/login", new PasswordLoginRequest(
            username,
            newPassword,
            PasswordAuthApiTestHost.TenantId,
            PasswordAuthApiTestHost.Environment,
            Scopes: null,
            Audience: null));

        newLoginAttempt.StatusCode.ShouldBe(HttpStatusCode.OK);
        var newLoginBody = await newLoginAttempt.Content.ReadFromJsonAsync<TokenEnvelope>();
        newLoginBody.ShouldNotBeNull();
        newLoginBody!.PasswordChangeRequired.ShouldNotBeNull();
        newLoginBody.PasswordChangeRequired!.Value.ShouldBeFalse();

        var stored = await _host.Users.FindByUsernameAsync(PasswordAuthApiTestHost.TenantId, username);
        stored.ShouldNotBeNull();
        stored!.PasswordChangeRequired.ShouldBeFalse();
        stored.FailedLoginCount.ShouldBe(0);
        stored.LockoutEndUtc.ShouldBeNull();
    }

    private sealed class TokenEnvelope
    {
        public string? AccessToken { get; set; }
        public string? TokenType { get; set; }
        public int? ExpiresIn { get; set; }
        public string? RefreshToken { get; set; }
        public bool? PasswordChangeRequired { get; set; }
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
