using System;
using System.Collections.Generic;
using System.Linq;
using Croniq.Api;
using Croniq.Api.Security;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Routing;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class EndpointGuardMetadataTests
{
    [Fact]
    public void All_non_anonymous_endpoints_have_explicit_guard_metadata()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(ApiHostingExtensions).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        builder.Configuration.AddInMemoryCollection(new Dictionary<string, string?>
        {
            ["Croniq:Api:RequestsPerMinute"] = "0"
        });

        builder.Services.AddRouting();
        builder.Services.AddOptions();
        builder.Services.Configure<CroniqApiOptions>(builder.Configuration.GetSection("Croniq:Api"));

        builder.Services.AddSingleton<ICallerContextAccessor, CallerContextAccessor>();
        builder.Services.AddSingleton<TestCallerContextFactory>();
        builder.Services.AddSingleton<ICallerContextFactory>(sp => sp.GetRequiredService<TestCallerContextFactory>());

        builder.Services.AddSingleton<TenantRateLimitDecider>();

        var app = builder.Build();
        app.UseCroniqApi();

        var endpoints = ((IEndpointRouteBuilder)app).DataSources
            .SelectMany(source => source.Endpoints)
            .OfType<RouteEndpoint>()
            .ToArray();

        endpoints.Length.ShouldBeGreaterThan(0);

        var anonymousPrefixes = new[]
        {
            "/health",
            "/webhooks",
            "/auth/login",
            "/auth/refresh",
            "/auth/logout",
            "/api/health",
            "/api/webhooks",
            "/api/auth/login",
            "/api/auth/refresh",
            "/api/auth/logout"
        };

        static bool IsAnonymous(string rawText, string[] prefixes)
        {
            if (string.IsNullOrWhiteSpace(rawText))
            {
                return false;
            }

            foreach (var prefix in prefixes)
            {
                if (rawText.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
            }

            return false;
        }

        var missing = new List<string>();
        var weakTenantGuards = new List<string>();

        foreach (var endpoint in endpoints)
        {
            var raw = endpoint.RoutePattern.RawText ?? string.Empty;

            if (IsAnonymous(raw, anonymousPrefixes))
            {
                continue;
            }

            var guard = endpoint.Metadata.OfType<EndpointAuthExtensions.ICroniqAuthEndpointGuardMetadata>().FirstOrDefault();
            if (guard is null)
            {
                missing.Add(raw);
                continue;
            }

            if (raw.StartsWith("/tenants/", StringComparison.OrdinalIgnoreCase)
                && raw.Contains("{tenantId}", StringComparison.OrdinalIgnoreCase)
                && guard.Kind == EndpointAuthExtensions.CroniqAuthGuardKind.Caller)
            {
                weakTenantGuards.Add(raw);
            }
        }

        missing.ShouldBeEmpty($"Endpoints without guard metadata: {string.Join(", ", missing)}");
        weakTenantGuards.ShouldBeEmpty($"Tenant endpoints should not be guarded only by RequireCroniqCaller(): {string.Join(", ", weakTenantGuards)}");
    }
}
