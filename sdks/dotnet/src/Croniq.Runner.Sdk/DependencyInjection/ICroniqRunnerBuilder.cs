using Microsoft.Extensions.DependencyInjection;

namespace Croniq.Runner.Sdk.DependencyInjection;

/// <summary>
/// Fluent builder returned by <c>AddCroniqRunner(...)</c>. Use it to chain
/// <c>AddCroniqJob</c>, <c>AddCroniqDefaultHandler</c>, etc.
/// </summary>
public interface ICroniqRunnerBuilder
{
    /// <summary>The underlying service collection.</summary>
    IServiceCollection Services { get; }
}

internal sealed class CroniqRunnerBuilder(IServiceCollection services) : ICroniqRunnerBuilder
{
    public IServiceCollection Services { get; } = services;
}
