using System;
using Croniq.Api;
using Croniq.Api.Models;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class OptionsDefaultsTests
{
    [Fact]
    public void ApiOptions_Defaults_AreSet()
    {
        var options = new CroniqApiOptions();

        options.RequestsPerMinute.ShouldBe(60);
        options.AnonymousPathPrefixes.ShouldNotBeNull();
        options.TenantRateLimits.ShouldNotBeNull();
        options.TenantRateLimits.Comparer.ShouldBe(StringComparer.OrdinalIgnoreCase);
        options.RateLimiterCacheRetention.ShouldBe(TimeSpan.FromMinutes(10));
        options.RateLimiterCacheCleanupInterval.ShouldBe(TimeSpan.FromMinutes(2));

        var tenantOptions = new TenantRateLimitOptions();
        tenantOptions.RequestsPerMinute.ShouldBe(60);
    }

    [Fact]
    public void AuthOptions_Defaults_AreSet()
    {
        var options = new CroniqAuthOptions();

        options.Mode.ShouldBe("SqlServer");
        options.SqlServer.ShouldNotBeNull();
        options.InMemory.ShouldNotBeNull();
        options.InMemory.ApiKey.ShouldBe("dev-key");
        options.InMemory.TenantReference.ShouldBe("dev");
        options.InMemory.EnvironmentTag.ShouldBe("dev");
        options.Oidc.ShouldNotBeNull();
    }

    [Fact]
    public void SqlServerAuthOptions_AssignsValues()
    {
        var options = new SqlServerAuthOptions
        {
            ConnectionString = "Server=localhost;Database=Croniq;",
            MigrationsAssembly = "Croniq.Api",
            EnableDetailedErrors = true,
            EnableSensitiveDataLogging = false
        };

        options.ConnectionString.ShouldBe("Server=localhost;Database=Croniq;");
        options.MigrationsAssembly.ShouldBe("Croniq.Api");
        options.EnableDetailedErrors.ShouldBe(true);
        options.EnableSensitiveDataLogging.ShouldBe(false);
    }

    [Fact]
    public void PersistenceOptions_Defaults_AreSet()
    {
        var options = new CroniqPersistenceOptions();

        options.Mode.ShouldBe("InMemory");
        options.SqlServer.ShouldNotBeNull();
    }

    [Fact]
    public void SqlServerPersistenceNode_AssignsValues()
    {
        var options = new SqlServerPersistenceNode
        {
            ConnectionString = "Server=localhost;Database=Croniq;",
            MigrationsAssembly = "Croniq.Api",
            EnableDetailedErrors = true,
            EnableSensitiveDataLogging = false,
            LeaseDurationSeconds = 42,
            DeadLetterRetentionDays = 7,
            DeadLetterReasonMaxLength = 255
        };

        options.ConnectionString.ShouldBe("Server=localhost;Database=Croniq;");
        options.MigrationsAssembly.ShouldBe("Croniq.Api");
        options.EnableDetailedErrors.ShouldBe(true);
        options.EnableSensitiveDataLogging.ShouldBe(false);
        options.LeaseDurationSeconds.ShouldBe(42);
        options.DeadLetterRetentionDays.ShouldBe(7);
        options.DeadLetterReasonMaxLength.ShouldBe(255);
    }

    [Fact]
    public void PasswordLogoutRequest_AssignsValues()
    {
        var request = new PasswordLogoutRequest("refresh", "tenant-1");

        request.RefreshToken.ShouldBe("refresh");
        request.TenantReference.ShouldBe("tenant-1");
    }
}
