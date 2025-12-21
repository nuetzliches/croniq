using System;
using Croniq.Core.Scheduling;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public sealed class CronExpressionUnsupportedTests
{
    [Fact]
    public void GetTimeBefore_Throws_not_supported()
    {
        var expression = new CronExpression("0 */5 * * * ?");

        Should.Throw<NotSupportedException>(() => expression.GetTimeBefore(DateTimeOffset.UtcNow));
    }

    [Fact]
    public void GetFinalFireTime_Throws_not_supported()
    {
        var expression = new CronExpression("0 */5 * * * ?");

        Should.Throw<NotSupportedException>(() => expression.GetFinalFireTime());
    }
}
