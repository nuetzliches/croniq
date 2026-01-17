namespace Croniq.Core.Scheduling;

public sealed class CalendarEvaluationOptions
{
    internal const int DefaultMaxCandidateIterations = 10000;
    internal const int DefaultMaxLookaheadDays = 365;

    public int MaxCandidateIterations { get; set; } = DefaultMaxCandidateIterations;

    public int MaxLookaheadDays { get; set; } = DefaultMaxLookaheadDays;

    internal void Normalize()
    {
        if (MaxCandidateIterations <= 0)
        {
            MaxCandidateIterations = DefaultMaxCandidateIterations;
        }

        if (MaxLookaheadDays <= 0)
        {
            MaxLookaheadDays = DefaultMaxLookaheadDays;
        }
    }
}
