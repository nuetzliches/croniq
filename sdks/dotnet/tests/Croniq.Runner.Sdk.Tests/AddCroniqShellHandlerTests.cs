using Croniq.Runner.Sdk.DependencyInjection;
using Croniq.Runner.Sdk.ShellExec;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Registration semantics for <c>AddCroniqShellHandler</c> (issue #442):
/// the parameterless overload stays the documented catch-all opt-in, while
/// the scoped overload registers the handler for the listed job keys only.
/// </summary>
public class AddCroniqShellHandlerTests
{
    private static ICroniqRunnerBuilder NewRunnerBuilder(out ServiceCollection services)
    {
        services = new ServiceCollection();
        services.AddLogging();
        return services.AddCroniqRunner(opts =>
        {
            opts.ServerUrl = "https://example.test:4000";
            opts.ApiKey = "croniq_abc";
        });
    }

    [Fact]
    public void Parameterless_RegistersCatchAll()
    {
        var builder = NewRunnerBuilder(out var services);

        builder.AddCroniqShellHandler();

        using var provider = services.BuildServiceProvider();
        var reg = provider.GetServices<HandlerRegistration>()
            .ShouldHaveSingleItem();
        reg.HandlerType.ShouldBe(typeof(CroniqShellHandler));
        reg.IsDefault.ShouldBeTrue();
        reg.JobKey.ShouldBe(string.Empty);
    }

    [Fact]
    public void Scoped_RegistersOnlyTheGivenJobKeys()
    {
        var builder = NewRunnerBuilder(out var services);

        builder.AddCroniqShellHandler("deploy:run", "deploy:cleanup");

        using var provider = services.BuildServiceProvider();
        var regs = provider.GetServices<HandlerRegistration>()
            .Where(r => r.HandlerType == typeof(CroniqShellHandler))
            .ToArray();

        regs.Length.ShouldBe(2);
        regs.ShouldAllBe(r => !r.IsDefault);
        regs.Select(r => r.JobKey).ShouldBe(["deploy:run", "deploy:cleanup"]);
    }

    [Fact]
    public void Scoped_WithEmptyKeyArray_Throws()
    {
        var builder = NewRunnerBuilder(out _);

        Should.Throw<ArgumentException>(() => builder.AddCroniqShellHandler(Array.Empty<string>()));
    }

    [Fact]
    public void Scoped_WithEmptyKey_Throws()
    {
        var builder = NewRunnerBuilder(out _);

        Should.Throw<ArgumentException>(() => builder.AddCroniqShellHandler("deploy:run", ""));
    }

    [Fact]
    public void ConfigureOverload_BindsHandlerOptions()
    {
        var builder = NewRunnerBuilder(out var services);

        builder.AddCroniqShellHandler(o => o.AllowUnsafeEnvironment = true, "deploy:run");

        using var provider = services.BuildServiceProvider();
        provider.GetRequiredService<IOptions<CroniqShellHandlerOptions>>()
            .Value.AllowUnsafeEnvironment.ShouldBeTrue();
    }

    [Fact]
    public void HandlerOptions_DefaultToSafeEnvironment()
    {
        var builder = NewRunnerBuilder(out var services);

        builder.AddCroniqShellHandler("deploy:run");

        using var provider = services.BuildServiceProvider();
        provider.GetRequiredService<IOptions<CroniqShellHandlerOptions>>()
            .Value.AllowUnsafeEnvironment.ShouldBeFalse();
    }

    [Fact]
    public void ScopedHandler_IsResolvableFromDi()
    {
        var builder = NewRunnerBuilder(out var services);

        builder.AddCroniqShellHandler("deploy:run");

        using var provider = services.BuildServiceProvider();
        using var scope = provider.CreateScope();
        scope.ServiceProvider.GetRequiredService<CroniqShellHandler>().ShouldNotBeNull();
    }
}
