using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Base-URL transport security (#440). <c>https://</c> is always accepted;
/// <c>http://</c> only for a loopback host — the documented
/// <c>http://localhost:4000</c> quickstart path — or behind an explicit
/// <c>AllowInsecureHttp</c>, which additionally logs one loud warning.
/// Enforced when the options are materialised, i.e. at startup, not on the
/// first request.
/// </summary>
public class ServerUrlSecurityTests
{
    public static TheoryData<string> AcceptedUrls =>
    [
        "https://croniq.example.com",
        "https://croniq.example.com:4000",
        "http://localhost:4000",
        "http://LOCALHOST:4000",
        "http://127.0.0.1:4000",
        "http://127.10.20.30:4000",
        "http://[::1]:4000",
    ];

    public static TheoryData<string> RejectedUrls =>
    [
        "http://croniq.example.com",
        "http://croniq.example.com:4000",
        "http://10.0.0.5:4000",
        "http://[2001:db8::1]:4000",
    ];

    [Theory]
    [MemberData(nameof(AcceptedUrls))]
    public void RunnerOptions_AcceptSecureOrLoopbackUrl(string serverUrl)
    {
        var opts = ResolveRunnerOptions(o => o.ServerUrl = serverUrl, out _);
        opts.ServerUrl.ShouldBe(serverUrl);
    }

    [Theory]
    [MemberData(nameof(AcceptedUrls))]
    public void ClientOptions_AcceptSecureOrLoopbackUrl(string serverUrl)
    {
        var opts = ResolveClientOptions(o => o.ServerUrl = serverUrl, out _);
        opts.ServerUrl.ShouldBe(serverUrl);
    }

    [Theory]
    [MemberData(nameof(RejectedUrls))]
    public void RunnerOptions_RejectNonLoopbackCleartextUrl(string serverUrl)
    {
        var ex = Should.Throw<OptionsValidationException>(
            () => ResolveRunnerOptions(o => o.ServerUrl = serverUrl, out _));

        // Actionable: names the URL and the opt-in property.
        ex.Message.ShouldContain(serverUrl);
        ex.Message.ShouldContain("AllowInsecureHttp");
    }

    [Theory]
    [MemberData(nameof(RejectedUrls))]
    public void ClientOptions_RejectNonLoopbackCleartextUrl(string serverUrl)
    {
        var ex = Should.Throw<OptionsValidationException>(
            () => ResolveClientOptions(o => o.ServerUrl = serverUrl, out _));

        ex.Message.ShouldContain(serverUrl);
        ex.Message.ShouldContain("AllowInsecureHttp");
    }

    [Fact]
    public void RunnerOptions_KeepTheQuickstartDefaultWorking()
    {
        var opts = ResolveRunnerOptions(_ => { }, out var logs);

        opts.ServerUrl.ShouldBe("http://localhost:4000");
        logs.Warnings.ShouldBeEmpty();
    }

    [Fact]
    public void ClientOptions_KeepTheQuickstartDefaultWorking()
    {
        var opts = ResolveClientOptions(_ => { }, out var logs);

        opts.ServerUrl.ShouldBe("http://localhost:4000");
        logs.Warnings.ShouldBeEmpty();
    }

    [Fact]
    public void RunnerOptions_RejectUnsupportedScheme()
    {
        var ex = Should.Throw<OptionsValidationException>(
            () => ResolveRunnerOptions(o => o.ServerUrl = "ftp://croniq.example.com", out _));

        ex.Message.ShouldContain("unsupported scheme");
    }

    [Fact]
    public void RunnerOptions_AcceptCleartextUrlWithOptInAndWarnOnce()
    {
        var opts = ResolveRunnerOptions(
            o =>
            {
                o.ServerUrl = "http://croniq.example.com:4000";
                o.AllowInsecureHttp = true;
            },
            out var logs);

        opts.ServerUrl.ShouldBe("http://croniq.example.com:4000");
        logs.Warnings.Count.ShouldBe(1);
        logs.Warnings[0].ShouldContain("SECURITY");
        logs.Warnings[0].ShouldContain("cleartext");
        logs.Warnings[0].ShouldContain("http://croniq.example.com:4000");
    }

    [Fact]
    public void ClientOptions_AcceptCleartextUrlWithOptInAndWarnOnce()
    {
        var opts = ResolveClientOptions(
            o =>
            {
                o.ServerUrl = "http://croniq.example.com:4000";
                o.AllowInsecureHttp = true;
            },
            out var logs);

        opts.AllowInsecureHttp.ShouldBeTrue();
        logs.Warnings.Count.ShouldBe(1);
        logs.Warnings[0].ShouldContain("SECURITY");
    }

    private static CroniqRunnerOptions ResolveRunnerOptions(
        Action<CroniqRunnerOptions> configure,
        out WarningCollector logs)
    {
        var collector = new WarningCollector();
        var services = new ServiceCollection();
        services.AddLogging(b => b.AddProvider(collector).SetMinimumLevel(LogLevel.Warning));
        services.AddCroniqRunner(configure);

        using var provider = services.BuildServiceProvider();
        logs = collector;
        return provider.GetRequiredService<IOptions<CroniqRunnerOptions>>().Value;
    }

    private static CroniqClientOptions ResolveClientOptions(
        Action<CroniqClientOptions> configure,
        out WarningCollector logs)
    {
        var collector = new WarningCollector();
        var services = new ServiceCollection();
        services.AddLogging(b => b.AddProvider(collector).SetMinimumLevel(LogLevel.Warning));
        services.AddCroniqClient(configure);

        using var provider = services.BuildServiceProvider();
        logs = collector;
        return provider.GetRequiredService<IOptions<CroniqClientOptions>>().Value;
    }

    /// <summary>Captures warning-level log messages emitted during a test.</summary>
    private sealed class WarningCollector : ILoggerProvider, ILogger
    {
        public List<string> Warnings { get; } = [];

        public ILogger CreateLogger(string categoryName) => this;

        public IDisposable? BeginScope<TState>(TState state)
            where TState : notnull => null;

        public bool IsEnabled(LogLevel logLevel) => logLevel >= LogLevel.Warning;

        public void Log<TState>(
            LogLevel logLevel,
            EventId eventId,
            TState state,
            Exception? exception,
            Func<TState, Exception?, string> formatter)
        {
            if (logLevel >= LogLevel.Warning)
            {
                Warnings.Add(formatter(state, exception));
            }
        }

        public void Dispose()
        {
            // Nothing to release — the collector is owned by the test.
        }
    }
}
