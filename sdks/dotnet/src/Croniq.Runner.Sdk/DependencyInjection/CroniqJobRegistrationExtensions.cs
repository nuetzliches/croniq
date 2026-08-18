using System.Diagnostics.CodeAnalysis;

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
    public static ICroniqRunnerBuilder AddCroniqJob<[DynamicallyAccessedMembers(DynamicallyAccessedMemberTypes.PublicConstructors)] THandler>(
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
    public static ICroniqRunnerBuilder AddCroniqJob<[DynamicallyAccessedMembers(DynamicallyAccessedMemberTypes.PublicConstructors)] THandler>(
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
    public static ICroniqRunnerBuilder AddCroniqDefaultHandler<[DynamicallyAccessedMembers(DynamicallyAccessedMemberTypes.PublicConstructors)] THandler>(
        this ICroniqRunnerBuilder builder)
        where THandler : class, ICroniqJobHandler
    {
        builder.Services.TryAddScoped<THandler>();
        builder.Services.AddSingleton(new HandlerRegistration(JobKey: string.Empty, Schedule: null, Timeout: null, Description: null, HandlerType: typeof(THandler), HandlerDelegate: null, IsDefault: true));
        return builder;
    }

    /// <summary>
    /// Register the shell-exec handler (decodes <c>__runner_exec</c> metadata
    /// and spawns a subprocess) as the <b>catch-all default</b>: every job key
    /// the server dispatches to this runner is executed as a subprocess.
    /// Equivalent to running the generic Rust <c>croniq-shell-runner</c>, and
    /// therefore a deliberate opt-in that trusts the server with shell-exec
    /// over any job it routes here. Prefer the scoped
    /// <see cref="AddCroniqShellHandler(ICroniqRunnerBuilder, string[])"/>
    /// overload, which grants the capability only for the job keys listed.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqShellHandler(this ICroniqRunnerBuilder builder) =>
        AddShellHandlerCore(builder, configure: null, jobKeys: []);

    /// <summary>
    /// Register the shell-exec handler for the given job keys only
    /// (preferred). Jobs with other keys are unaffected — the server-supplied
    /// <c>__runner_exec</c> payload is executed as a subprocess exclusively
    /// for the keys listed here.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqShellHandler(this ICroniqRunnerBuilder builder, params string[] jobKeys)
    {
        ValidateJobKeys(jobKeys);
        return AddShellHandlerCore(builder, configure: null, jobKeys);
    }

    /// <summary>
    /// Register the shell-exec handler as the catch-all default (see
    /// <see cref="AddCroniqShellHandler(ICroniqRunnerBuilder)"/>) with
    /// explicit <see cref="CroniqShellHandlerOptions"/> configuration.
    /// Prefer a scoped overload where possible.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqShellHandler(this ICroniqRunnerBuilder builder, Action<CroniqShellHandlerOptions> configure)
    {
        ArgumentNullException.ThrowIfNull(configure);
        return AddShellHandlerCore(builder, configure, jobKeys: []);
    }

    /// <summary>
    /// Register the shell-exec handler for the given job keys only
    /// (preferred), with explicit <see cref="CroniqShellHandlerOptions"/>
    /// configuration.
    /// </summary>
    public static ICroniqRunnerBuilder AddCroniqShellHandler(this ICroniqRunnerBuilder builder, Action<CroniqShellHandlerOptions> configure, params string[] jobKeys)
    {
        ArgumentNullException.ThrowIfNull(configure);
        ValidateJobKeys(jobKeys);
        return AddShellHandlerCore(builder, configure, jobKeys);
    }

    private static ICroniqRunnerBuilder AddShellHandlerCore(ICroniqRunnerBuilder builder, Action<CroniqShellHandlerOptions>? configure, string[] jobKeys)
    {
        builder.Services.TryAddScoped<CroniqShellHandler>();
        builder.Services.AddOptions<CroniqShellHandlerOptions>();
        if (configure is not null)
        {
            builder.Services.Configure(configure);
        }

        if (jobKeys.Length == 0)
        {
            builder.Services.AddSingleton(new HandlerRegistration(JobKey: string.Empty, Schedule: null, Timeout: null, Description: null, HandlerType: typeof(CroniqShellHandler), HandlerDelegate: null, IsDefault: true));
        }
        else
        {
            foreach (var jobKey in jobKeys)
            {
                builder.Services.AddSingleton(new HandlerRegistration(jobKey, Schedule: null, Timeout: null, Description: null, HandlerType: typeof(CroniqShellHandler), HandlerDelegate: null, IsDefault: false));
            }
        }
        return builder;
    }

    private static void ValidateJobKeys(string[] jobKeys)
    {
        ArgumentNullException.ThrowIfNull(jobKeys);
        if (jobKeys.Length == 0)
        {
            throw new ArgumentException(
                "At least one job key is required. Use the parameterless AddCroniqShellHandler() overload to opt into the catch-all registration.",
                nameof(jobKeys));
        }
        foreach (var jobKey in jobKeys)
        {
            ArgumentException.ThrowIfNullOrEmpty(jobKey, nameof(jobKeys));
        }
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
