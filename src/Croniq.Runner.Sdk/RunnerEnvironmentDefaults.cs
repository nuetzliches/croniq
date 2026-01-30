namespace Croniq.Runner;

public sealed record RunnerEnvironmentDefaults
{
    public string? RunnerApiKeyEnv { get; init; }
    public string? DefaultRunnerId { get; init; }
    public string? RunnerApiKeyDefaultRunnerId { get; init; }
}
