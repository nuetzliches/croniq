using Croniq.Runner.Sdk.Internal;
using Croniq.Runner.Sdk.ShellExec;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.DependencyInjection.Extensions;

namespace Croniq.Runner.Sdk.DependencyInjection;

/// <summary>
/// Job-handler registration helpers. Use after <c>AddCroniqRunner(...)</c>.
/// </summary>
public static class CroniqJobRegistrationExtensions
{
    /// <summary>Register a delegate handler for a specific job key.</summary>
    public static ICroniqRunnerBuilder AddCroniqJob(
        this ICroniqRunnerBuilder builder,
        string jobKey,
        Func<CroniqExecutionContext, CancellationToken, Task> handler)
    {
        ArgumentException.ThrowIfNullOrEmpty(jobKey);
        ArgumentNullException.ThrowIfNull(handler);

        builder.Services.AddSingleton(new HandlerRegistration(jobKey, Schedule: null, Timeout: null, Description: null, HandlerType: null, HandlerDelegate: handler, IsDefault: false));
        return builder;
    }

    /// <summary>Register a delegate handler with self-registration of a schedule via <c>POST /v1/jobs/register</c>.</summary>
    public static ICroniqRunnerBuilder AddCroniqJob(
        this ICroniqRunnerBuilder builder,
        string jobKey,
        string schedule,
        Func<CroniqExecutionContext, CancellationToken, Task> handler)
    {
        ArgumentException.ThrowIfNullOrEmpty(jobKey);
        ArgumentException.ThrowIfNullOrEmpty(schedule);
        ArgumentNullException.ThrowIfNull(handler);

        builder.Services.AddSingleton(new HandlerRegistration(jobKey, schedule, Timeout: null, Description: null, HandlerType: null, HandlerDelegate: handler, IsDefault: false));
        return builder;
    }

    /// <summary>Register an <see cref="ICroniqJobHandler"/> implementation for a job key.</summary>
    public static ICroniqRunnerBuilder AddCroniqJob<THandler>(
        this ICroniqRunnerBuilder builder,
        string jobKey)
        where THandler : class, ICroniqJobHandler
    {
        ArgumentException.ThrowIfNullOrEmpty(jobKey);
        builder.Services.TryAddScoped<THandler>();
        builder.Services.AddSingleton(new HandlerRegistration(jobKey, Schedule: null, Timeout: null, Description: null, HandlerType: typeof(THandler), HandlerDelegate: null, IsDefault: false));
        return builder;
    }

    /// <summary>Register an <see cref="ICroniqJobHandler"/> with a schedule (self-register on startup).</summary>
    public static ICroniqRunnerBuilder AddCroniqJob<THandler>(
        this ICroniqRunnerBuilder builder,
        string jobKey,
        string schedule)
        where THandler : class, ICroniqJobHandler
    {
        ArgumentException.ThrowIfNullOrEmpty(jobKey);
        ArgumentException.ThrowIfNullOrEmpty(schedule);
        builder.Services.TryAddScoped<THandler>();
        builder.Services.AddSingleton(new HandlerRegistration(jobKey, schedule, Timeout: null, Description: null, HandlerType: typeof(THandler), HandlerDelegate: null, IsDefault: false));
        return builder;
    }

    /// <summary>Register a catch-all delegate handler invoked when no specific handler matches.</summary>
    public static ICroniqRunnerBuilder AddCroniqDefaultHandler(
        this ICroniqRunnerBuilder builder,
        Func<CroniqExecutionContext, CancellationToken, Task> handler)
    {
        ArgumentNullException.ThrowIfNull(handler);
        builder.Services.AddSingleton(new HandlerRegistration(JobKey: string.Empty, Schedule: null, Timeout: null, Description: null, HandlerType: null, HandlerDelegate: handler, IsDefault: true));
        return builder;
    }

    /// <summary>Register an <see cref="ICroniqJobHandler"/> as the catch-all.</summary>
    public static ICroniqRunnerBuilder AddCroniqDefaultHandler<THandler>(
        this ICroniqRunnerBuilder builder)
        where THandler : class, ICroniqJobHandler
    {
        builder.Services.TryAddScoped<THandler>();
        builder.Services.AddSingleton(new HandlerRegistration(JobKey: string.Empty, Schedule: null, Timeout: null, Description: null, HandlerType: typeof(THandler), HandlerDelegate: null, IsDefault: true));
        return builder;
    }

    /// <summary>
    /// Register a default shell-exec handler that decodes <c>__runner_exec</c>
    /// metadata and spawns a subprocess. Equivalent to the Rust
    /// <c>croniq-shell-runner</c> crate.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqShellHandler(this ICroniqRunnerBuilder builder)
    {
        builder.Services.TryAddScoped<CroniqShellHandler>();
        builder.Services.AddSingleton(new HandlerRegistration(JobKey: string.Empty, Schedule: null, Timeout: null, Description: null, HandlerType: typeof(CroniqShellHandler), HandlerDelegate: null, IsDefault: true));
        return builder;
    }

    /// <summary>
    /// Hook called by the SDK at runtime to apply all
    /// <see cref="HandlerRegistration"/>s to the registry. Internal.
    /// </summary>
    internal static void PopulateRegistry(IServiceProvider services)
    {
        var registry = services.GetRequiredService<CroniqHandlerRegistry>();
        foreach (var reg in services.GetServices<HandlerRegistration>())
        {
            reg.ApplyTo(registry);
        }
    }
}

internal sealed record HandlerRegistration(
    string JobKey,
    string? Schedule,
    string? Timeout,
    string? Description,
    Type? HandlerType,
    Func<CroniqExecutionContext, CancellationToken, Task>? HandlerDelegate,
    bool IsDefault)
{
    public void ApplyTo(CroniqHandlerRegistry registry)
    {
        if (IsDefault)
        {
            if (HandlerDelegate is not null)
            {
                registry.SetDefaultDelegate(HandlerDelegate);
            }
            else if (HandlerType is not null)
            {
                registry.SetDefaultInterface(HandlerType);
            }
            return;
        }

        if (HandlerDelegate is not null)
        {
            registry.RegisterDelegate(JobKey, HandlerDelegate);
        }
        else if (HandlerType is not null)
        {
            registry.RegisterInterface(JobKey, HandlerType);
        }

        if (Schedule is not null)
        {
            registry.AddSelfRegisterSchedule(JobKey, Schedule, Timeout, Description);
        }
    }
}
