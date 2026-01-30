using System.Collections.Generic;
using Croniq.Runner;
using Shouldly;
using Xunit;

namespace Croniq.Runner.Sdk.Tests;

public sealed class RunnerConfigEnvironmentDefaultsTests
{
    [Fact]
    public void FromEnvironment_UsesRunnerApiKey_WhenApiKeyMissing()
    {
        var env = CreateBaseEnv();
        env["CRONIQ_RUNNER_NODE_API_KEY"] = "ak_node";

        var defaults = new RunnerEnvironmentDefaults
        {
            RunnerApiKeyEnv = "CRONIQ_RUNNER_NODE_API_KEY",
            DefaultRunnerId = "default",
            RunnerApiKeyDefaultRunnerId = "node-default"
        };

        var config = RunnerConfig.FromEnvironment(env, defaults);

        config.ApiKey.ShouldBe("ak_node");
        config.RunnerId.ShouldBe("node-default");
    }

    [Fact]
    public void FromEnvironment_UsesDefaultRunnerId_WhenMissing()
    {
        var env = CreateBaseEnv();
        env["CRONIQ_API_KEY"] = "ak_default";

        var defaults = new RunnerEnvironmentDefaults
        {
            DefaultRunnerId = "default"
        };

        var config = RunnerConfig.FromEnvironment(env, defaults);

        config.RunnerId.ShouldBe("default");
    }

    [Fact]
    public void FromEnvironment_OverridesDefaultRunnerId_WhenRunnerApiKeyPresent()
    {
        var env = CreateBaseEnv();
        env["CRONIQ_RUNNER_ID"] = "default";
        env["CRONIQ_RUNNER_NODE_API_KEY"] = "ak_node";

        var defaults = new RunnerEnvironmentDefaults
        {
            RunnerApiKeyEnv = "CRONIQ_RUNNER_NODE_API_KEY",
            DefaultRunnerId = "default",
            RunnerApiKeyDefaultRunnerId = "node-default"
        };

        var config = RunnerConfig.FromEnvironment(env, defaults);

        config.RunnerId.ShouldBe("node-default");
    }

    private static Dictionary<string, string?> CreateBaseEnv() => new()
    {
        ["CRONIQ_API_BASEURL"] = "http://localhost:5080",
        ["CRONIQ_TENANT_ID"] = "default",
        ["CRONIQ_ENVIRONMENT"] = "dev"
    };
}
