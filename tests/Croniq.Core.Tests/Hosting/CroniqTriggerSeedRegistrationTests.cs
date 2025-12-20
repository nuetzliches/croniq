using System;
using Croniq.Core.Hosting;
using Croniq.Sdk;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Hosting;

public class CroniqTriggerSeedRegistrationTests
{
    [Fact]
    public void Constructor_Throws_OnMissingInputs()
    {
        var attribute = new CroniqJobAttribute("samples", "demo");

        Should.Throw<ArgumentNullException>(() => new CroniqTriggerSeedRegistration(null!, "* * * * *"));
        Should.Throw<ArgumentException>(() => new CroniqTriggerSeedRegistration(attribute, " "));
    }

    [Fact]
    public void Constructor_SetsDefaults()
    {
        var attribute = new CroniqJobAttribute("samples", "demo");

        var registration = new CroniqTriggerSeedRegistration(attribute, "* * * * *");

        registration.JobAttribute.ShouldBe(attribute);
        registration.CronExpression.ShouldBe("* * * * *");
        registration.Enabled.ShouldBeTrue();
    }
}
