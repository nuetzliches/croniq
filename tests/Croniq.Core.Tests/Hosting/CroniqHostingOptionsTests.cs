using Croniq.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public class CroniqHostingOptionsTests
{
    [Fact]
    public void AuthOptions_Defaults_AreSet()
    {
        var options = new CroniqAuthOptions();

        options.Mode.ShouldBe("SqlServer");
        options.SqlServer.ShouldNotBeNull();
        options.InMemory.ShouldNotBeNull();
        options.InMemory.ApiKey.ShouldBe("dev-key");
        options.InMemory.TenantId.ShouldBe("default");
        options.InMemory.EnvironmentTag.ShouldBe("dev");
    }

    [Fact]
    public void SqlServerAuthOptions_AssignsValues()
    {
        var options = new SqlServerAuthOptions
        {
            ConnectionString = "Server=localhost;Database=Croniq;",
            MigrationsAssembly = "Croniq.Hosting",
            EnableDetailedErrors = true,
            EnableSensitiveDataLogging = false
        };

        options.ConnectionString.ShouldBe("Server=localhost;Database=Croniq;");
        options.MigrationsAssembly.ShouldBe("Croniq.Hosting");
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
            MigrationsAssembly = "Croniq.Hosting",
            EnableDetailedErrors = true,
            EnableSensitiveDataLogging = false,
            LeaseDurationSeconds = 30,
            DeadLetterRetentionDays = 14,
            DeadLetterReasonMaxLength = 200
        };

        options.ConnectionString.ShouldBe("Server=localhost;Database=Croniq;");
        options.MigrationsAssembly.ShouldBe("Croniq.Hosting");
        options.EnableDetailedErrors.ShouldBe(true);
        options.EnableSensitiveDataLogging.ShouldBe(false);
        options.LeaseDurationSeconds.ShouldBe(30);
        options.DeadLetterRetentionDays.ShouldBe(14);
        options.DeadLetterReasonMaxLength.ShouldBe(200);
    }
}
