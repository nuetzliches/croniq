using System.Collections.Generic;
using System.Text;

namespace Croniq.Core.Scheduling;

internal sealed class CronExpressionSummary
{
    private readonly IEnumerable<int> _seconds;
    private readonly IEnumerable<int> _minutes;
    private readonly IEnumerable<int> _hours;
    private readonly IEnumerable<int> _daysOfMonth;
    private readonly IEnumerable<int> _months;
    private readonly IEnumerable<int> _daysOfWeek;
    private readonly bool _lastDayOfWeek;
    private readonly bool _nearestWeekday;
    private readonly int _nthdayOfWeek;
    private readonly bool _lastDayOfMonth;
    private readonly bool _calendarDayOfWeek;
    private readonly bool _calendarDayOfMonth;
    private readonly IEnumerable<int> _years;

    public CronExpressionSummary(
        IEnumerable<int> seconds,
        IEnumerable<int> minutes,
        IEnumerable<int> hours,
        IEnumerable<int> daysOfMonth,
        IEnumerable<int> months,
        IEnumerable<int> daysOfWeek,
        bool lastDayOfWeek,
        bool nearestWeekday,
        int nthdayOfWeek,
        bool lastDayOfMonth,
        bool calendarDayOfWeek,
        bool calendarDayOfMonth,
        IEnumerable<int> years)
    {
        _seconds = seconds;
        _minutes = minutes;
        _hours = hours;
        _daysOfMonth = daysOfMonth;
        _months = months;
        _daysOfWeek = daysOfWeek;
        _lastDayOfWeek = lastDayOfWeek;
        _nearestWeekday = nearestWeekday;
        _nthdayOfWeek = nthdayOfWeek;
        _lastDayOfMonth = lastDayOfMonth;
        _calendarDayOfWeek = calendarDayOfWeek;
        _calendarDayOfMonth = calendarDayOfMonth;
        _years = years;
    }

    public override string ToString()
    {
        var builder = new StringBuilder();
        builder.Append("seconds: ").AppendLine(string.Join(",", _seconds));
        builder.Append("minutes: ").AppendLine(string.Join(",", _minutes));
        builder.Append("hours: ").AppendLine(string.Join(",", _hours));
        builder.Append("daysOfMonth: ").AppendLine(string.Join(",", _daysOfMonth));
        builder.Append("months: ").AppendLine(string.Join(",", _months));
        builder.Append("daysOfWeek: ").AppendLine(string.Join(",", _daysOfWeek));
        builder.Append("lastDayOfWeek: ").AppendLine(_lastDayOfWeek.ToString());
        builder.Append("nearestWeekday: ").AppendLine(_nearestWeekday.ToString());
        builder.Append("nthdayOfWeek: ").AppendLine(_nthdayOfWeek.ToString());
        builder.Append("lastDayOfMonth: ").AppendLine(_lastDayOfMonth.ToString());
        builder.Append("calendarDayOfWeek: ").AppendLine(_calendarDayOfWeek.ToString());
        builder.Append("calendarDayOfMonth: ").AppendLine(_calendarDayOfMonth.ToString());
        builder.Append("years: ").AppendLine(string.Join(",", _years));
        return builder.ToString();
    }
}
