using System;
using Croniq.Core.Scheduling;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public class TriggerConstantsTests
{
    [Fact]
    public void Defaults_AreStable()
    {
        TriggerConstants.DefaultPriority.ShouldBe(5);
        TriggerConstants.EarliestYear.ShouldBe(1970);

        var currentYear = DateTime.UtcNow.Year;
        TriggerConstants.YearToGiveUpSchedulingAt.ShouldBeGreaterThanOrEqualTo(currentYear + 99);
    }
}
