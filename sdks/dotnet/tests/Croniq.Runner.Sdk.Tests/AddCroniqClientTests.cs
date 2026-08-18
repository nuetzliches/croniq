using System.Net;
using System.Text;

using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;

using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;

using Shouldly;

namespace Croniq.Runner.Sdk.Tests;

/// <summary>
/// DI coverage for <c>AddCroniqClient(...)</c>: registration, idempotency
/// (mirrors the <c>AddCroniqRunner</c> guarantees from #221/#223), and
/// credential isolation from the runner registration.
/// </summary>
public class AddCroniqClientTests
{
    [Fact]
    public void BindsOptionsFromConfigSection()
    {
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Client:ServerUrl"] = "https://example.test:4000",
                ["Croniq:Client:ApiKey"] = "croniq_trigger_key",
            })
            .Build();

        var services = new ServiceCollection();
        services.AddCroniqClient(config.GetSection(CroniqClientOptions.SectionName));

        using var provider = services.BuildServiceProvider();
        var opts = provider.GetRequiredService<IOptions<CroniqClientOptions>>().Value;

        opts.ServerUrl.ShouldBe("https://example.test:4000");
        opts.ApiKey.ShouldBe("croniq_trigger_key");
        provider.GetRequiredService<ICroniqTriggerClient>().ShouldNotBeNull();
    }

    [Fact]
    public async Task TriggerRequestCarriesClientCredentialsExactlyOnce()
    {
        // Register twice (idempotency) AND register the runner with different
        // credentials — the trigger call must carry the client's own key,
        // exactly once (no comma-joined header from duplicate auth handlers).
        var capture = new CaptureHandler();
        var services = new ServiceCollection();
        services.AddLogging();
        services.AddCroniqRunner(opts =>
        {
            opts.ServerUrl = "https://runner.test:4000";
            opts.ApiKey = "croniq_runner_key";
        });
        services.AddCroniqClient(opts =>
        {
            opts.ServerUrl = "https://example.test:4000";
            opts.ApiKey = "croniq_trigger_key";
        });
        services.AddCroniqClient();
        services.ConfigureHttpClientDefaults(b => b.ConfigurePrimaryHttpMessageHandler(() => capture));

        using var provider = services.BuildServiceProvider();
        var client = provider.GetRequiredService<ICroniqTriggerClient>();

        var result = await client.TriggerAsync("billing:invoice-generate");

        var headers = capture.LastRequest!.Headers.GetValues("Authorization").ToArray();
        headers.ShouldHaveSingleItem().ShouldBe("ApiKey croniq_trigger_key");
        capture.LastRequest.RequestUri!.ShouldBe(new Uri("https://example.test:4000/v1/trigger"));
        result.ExecutionId.ShouldBe("exec-1");
    }

    [Fact]
    public void Twice_RegistersOptionsValidationOnlyOnce()
    {
        var services = new ServiceCollection();
        services.AddCroniqClient(opts => opts.ServerUrl = "https://example.test:4000");
        var countAfterFirst = services.Count;
        services.AddCroniqClient(opts => opts.ServerUrl = "https://other.test:4000");

        // Second call must be a no-op for the shared setup.
        services.Count.ShouldBe(countAfterFirst);
    }

    private sealed class CaptureHandler : HttpMessageHandler
    {
        public HttpRequestMessage? LastRequest { get; private set; }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            LastRequest = request;
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK)
            {
                Content = new StringContent(
                    """{"execution_id":"exec-1","queued":0}""",
                    Encoding.UTF8,
                    "application/json"),
            });
        }
    }
}
