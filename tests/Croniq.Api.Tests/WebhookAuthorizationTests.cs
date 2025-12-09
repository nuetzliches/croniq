using Croniq.Api;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using FluentAssertions;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Http.HttpResults;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookAuthorizationTests
{
    [Fact]
    public void Ensure_returns_null_when_tenant_environment_and_scope_match()
    {
        var accessor = CreateAccessor(
            tenantId: "tenant-a",
            environment: "dev",
            scopes: new[] { CroniqScopes.WebhooksRead, CroniqScopes.WebhooksWrite });

        var result = WebhookAuthorization.Ensure(accessor, "tenant-a", "dev", WebhookAuthorization.WebhookScopes.Read);

        result.Should().BeNull();
    }

    [Fact]
    public void Ensure_returns_problem_when_tenant_does_not_match()
    {
        var accessor = CreateAccessor(
            tenantId: "tenant-a",
            environment: "dev",
            scopes: new[] { CroniqScopes.WebhooksRead });

        var result = WebhookAuthorization.Ensure(accessor, "tenant-b", "dev", WebhookAuthorization.WebhookScopes.Read);

        result.Should().BeOfType<ProblemHttpResult>()
            .Which.StatusCode.Should().Be(StatusCodes.Status403Forbidden);
    }

    [Fact]
    public void Ensure_returns_problem_when_environment_scope_is_more_restrictive()
    {
        var accessor = CreateAccessor(
            tenantId: "tenant-a",
            environment: "dev",
            scopes: new[] { CroniqScopes.WebhooksRead });

        var result = WebhookAuthorization.Ensure(accessor, "tenant-a", "qa", WebhookAuthorization.WebhookScopes.Read);

        result.Should().BeOfType<ProblemHttpResult>()
            .Which.StatusCode.Should().Be(StatusCodes.Status403Forbidden);
    }

    [Fact]
    public void Ensure_returns_problem_when_scope_missing()
    {
        var accessor = CreateAccessor(
            tenantId: "tenant-a",
            environment: "dev",
            scopes: new[] { CroniqScopes.WebhooksRead });

        var result = WebhookAuthorization.Ensure(accessor, "tenant-a", "dev", WebhookAuthorization.WebhookScopes.Write);

        result.Should().BeOfType<ProblemHttpResult>()
            .Which.StatusCode.Should().Be(StatusCodes.Status403Forbidden);
    }

    [Fact]
    public void Ensure_returns_unauthorized_when_caller_context_missing()
    {
        var accessor = new CallerContextAccessor();

        var result = WebhookAuthorization.Ensure(accessor, "tenant-a", "dev", WebhookAuthorization.WebhookScopes.Read);

        result.Should().BeOfType<ProblemHttpResult>()
            .Which.StatusCode.Should().Be(StatusCodes.Status401Unauthorized);
    }

    private static CallerContextAccessor CreateAccessor(string tenantId, string environment, IReadOnlyCollection<string> scopes)
    {
        return new CallerContextAccessor
        {
            Current = new CallerContext(
                tenantId,
                environment,
                CallerType.ApiKey,
                CallerId: "client",
                Scopes: scopes)
        };
    }
}
