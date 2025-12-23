using System;
using System.Reflection;
using Croniq.Core.Scheduling;
using Croniq.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public sealed class TriggerDefinitionValidatorTests
{
    [Fact]
    public void TryValidate_Fails_for_invalid_time_zone()
    {
        var definition = new CroniqTriggerSeedDefinition
        {
            JobKey = "ops:job",
            CronExpression = "0 */5 * * * ?",
            TimeZoneId = "Not/A/Zone"
        };

        var ok = TriggerDefinitionValidator.TryValidate(definition, scope: null, out _, out var error);

        ok.ShouldBeFalse();
        error.ShouldBe("TimeZoneId 'Not/A/Zone' is invalid.");
    }

    [Fact]
    public void TryValidate_Generates_url_safe_trigger_id()
    {
        var jobKey = "ops:job";
        var definition = new CroniqTriggerSeedDefinition
        {
            JobKey = jobKey,
            CronExpression = "0 */5 * * * ?",
            TimeZoneId = "UTC"
        };

        var ok = TriggerDefinitionValidator.TryValidate(definition, scope: null, out var result, out var error);

        ok.ShouldBeTrue(error);
        result.TriggerId.ShouldStartWith(jobKey + ":", Case.Sensitive);
        result.TriggerId.ShouldNotContain(" ");
        result.TriggerId.ShouldNotContain("/");
        result.TriggerId.ShouldNotContain("+");
        result.TriggerId.ShouldNotContain("=");
        result.TimeZoneId.ShouldBe("UTC");
    }

    [Fact]
    public void BuildTriggerId_Uses_hash_when_too_long()
    {
        var method = typeof(TriggerDefinitionValidator)
            .GetMethod("BuildTriggerId", BindingFlags.NonPublic | BindingFlags.Static);
        method.ShouldNotBeNull("BuildTriggerId should exist for hashing fallback.");

        var jobKey = "ops:job";
        var longExpression = new string('x', 700);
        var triggerId = (string)method!.Invoke(null, new object?[] { jobKey, longExpression, null })!;

        triggerId.ShouldContain(":hash-");
        triggerId.Length.ShouldBeLessThanOrEqualTo(512);
    }
}
