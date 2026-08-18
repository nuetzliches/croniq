using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;
using Croniq.Runner.Sdk.Hosting;
using Croniq.Runner.Sdk.Internal;

using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// Calling <c>AddCroniqRunner(...)</c> more than once on the same service
/// collection (e.g. from several feature modules) must not duplicate the
/// shared infrastructure. Regression coverage for the bug surfaced in
/// <see href="https://github.com/nuetzliches/croniq/issues/221">#221</see>.
/// </summary>
public class AddCroniqRunnerIdempotencyTests
{
    [Fact]
    public void TwiceWithSameConfigSection_DoesNotDuplicateOptionLists()
    {
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Runner:ServerUrl"] = "https://example.test:4000",
                ["Croniq:Runner:ApiKey"] = "croniq_abc",
                ["Croniq:Runner:Capabilities:0"] = "worker",
                ["Croniq:Runner:Tags:0"] = "lang=dotnet",
            })
            .Build();

        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqRunner(config.GetSection(CroniqRunnerOptions.SectionName));
        services.AddCroniqRunner(config.GetSection(CroniqRunnerOptions.SectionName));

        using var provider = services.BuildServiceProvider();
        var opts = provider.GetRequiredService<IOptions<CroniqRunnerOptions>>().Value;

        // Pre-fix the second Bind() appended a second copy of every IList
        // option, e.g. Capabilities = ["worker", "worker"].
        opts.Capabilities.ShouldHaveSingleItem().ShouldBe("worker");
        opts.Tags.ShouldHaveSingleItem().ShouldBe("lang=dotnet");
    }

    [Fact]
    public void Twice_RegistersExactlyOneHostedService()
    {
        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqRunner(opts =>
        {
            opts.ServerUrl = "https://example.test:4000";
            opts.ApiKey = "croniq_abc";
        });
        services.AddCroniqRunner();

        var hostedServiceCount = services.Count(d =>
            d.ServiceType == typeof(IHostedService)
            && d.ImplementationType == typeof(CroniqRunnerHostedService));

        hostedServiceCount.ShouldBe(1);
    }

    [Fact]
    public void Twice_ReturnsBuilderThatStillAcceptsJobRegistrations()
    {
        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqRunner(opts =>
        {
            opts.ServerUrl = "https://example.test:4000";
            opts.ApiKey = "croniq_abc";
        });

        // Second call from "another module" — must return a usable builder.
        var builder = services.AddCroniqRunner();
        builder.AddCroniqJob("demo:from-module-b", static (_, _) => Task.CompletedTask);

        using var provider = services.BuildServiceProvider();
        var registrations = provider.GetServices<HandlerRegistration>().ToArray();

        registrations.ShouldContain(r => r.JobKey == "demo:from-module-b");
    }

    [Fact]
    public async Task CroniqAuthHandler_OverwritesPreExistingAuthorizationHeader()
    {
        // Belt-and-braces against duplicate handler registrations or any
        // upstream code that already wrote an Authorization header — the
        // server's auth middleware splits on whitespace after the scheme,
        // so a comma-joined header turns into a hash lookup miss → 401.
        var monitor = new StubOptionsMonitor(new CroniqRunnerOptions
        {
            ServerUrl = "https://example.test",
            ApiKey = "croniq_abc",
        });
        var capture = new CaptureHandler();
        using var handler = new CroniqAuthHandler(monitor) { InnerHandler = capture };
        using var invoker = new HttpMessageInvoker(handler);

        using var req = new HttpRequestMessage(HttpMethod.Get, "https://example.test/v1/poll");
        req.Headers.TryAddWithoutValidation("Authorization", "ApiKey stale_value");

        using var _ = await invoker.SendAsync(req, CancellationToken.None);

        var headers = capture.LastRequest!.Headers.GetValues("Authorization").ToArray();
        headers.Length.ShouldBe(1);
        headers[0].ShouldBe("ApiKey croniq_abc");
    }

    private sealed class CaptureHandler : HttpMessageHandler
    {
        public HttpRequestMessage? LastRequest { get; private set; }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            LastRequest = request;
            return Task.FromResult(new HttpResponseMessage(System.Net.HttpStatusCode.OK));
        }
    }

    private sealed class StubOptionsMonitor(CroniqRunnerOptions value) : IOptionsMonitor<CroniqRunnerOptions>
    {
        public CroniqRunnerOptions CurrentValue { get; } = value;
        public CroniqRunnerOptions Get(string? name) => CurrentValue;
        public IDisposable? OnChange(Action<CroniqRunnerOptions, string?> listener) => null;
    }
}
