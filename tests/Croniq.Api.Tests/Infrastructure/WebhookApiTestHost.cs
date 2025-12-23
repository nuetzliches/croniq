using System.Collections.Generic;
using System.Net.Http;
using Croniq.Api;
using Croniq.Api.Security;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Croniq.Webhooks.InMemory;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Xunit;

namespace Croniq.Api.Tests.Infrastructure;

public sealed class WebhookApiTestHost : IAsyncLifetime
{
    private WebApplication? _app;

    public WebhookApiTestHost()
    {
        CallerFactory = new TestCallerContextFactory();
    }

    public const string TenantId = TestCallerContextFactory.DefaultTenantId;
    public const string Environment = TestCallerContextFactory.DefaultEnvironment;

    public HttpClient Client { get; private set; } = default!;

    public TestCallerContextFactory CallerFactory { get; }

    public InMemoryWebhookPersistenceProvider Webhooks { get; } = new();

    public InMemoryWebhookDeadLetterStore DeadLetters { get; } = new();

    public RecordingJobExecutionPipeline Pipeline { get; } = new();

    public TestExecutionLogReader ExecutionLogs { get; } = new();

    public TestExecutionHistoryReader ExecutionHistory { get; } = new();

    public InMemoryJobDeadLetterStore JobDeadLetters { get; } = new();

    public FakeJobRegistry Registry { get; } = new();

    public FakePolicyResolver Policies { get; } = new();

    public NoopJobPersistenceProvider JobStore { get; } = new();

    public FakeApiKeyStore ApiKeys { get; } = new();

    public TestTenantStore Tenants { get; } = new();

    public PartitionScope DefaultScope => new(TenantId, Environment);

    public async Task InitializeAsync()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(WebhookApiTestHost).Assembly.FullName,
            EnvironmentName = Environments.Development
        });

        builder.Configuration.AddInMemoryCollection(new Dictionary<string, string?>
        {
            ["Croniq:Api:RequestsPerMinute"] = "0",
            ["Croniq:Webhooks:RequestsPerMinute"] = "120",
            ["Croniq:Webhooks:Security:AllowUnsignedHooks"] = "true",
            ["Croniq:Auth:Tokens:Enabled"] = "true",
            ["Croniq:Auth:Tokens:Issuer"] = "https://itest.croniq",
            ["Croniq:Auth:Tokens:DefaultAudience"] = "cronqi-api",
            ["Croniq:Auth:Tokens:SigningKey"] = "Y3JvbmlxLWl0ZXN0LXNpZ25pbmcta2V5LTEyMzQ1Njc4OTA="
        });

        builder.Services.AddLogging();
        builder.Services.AddRouting();
        builder.Services.AddOptions();
        builder.Services.Configure<CroniqApiOptions>(builder.Configuration.GetSection("Croniq:Api"));
        builder.Services.Configure<CroniqOptions>(options =>
        {
            options.TenantReference = TenantId;
            options.EnvironmentTag = Environment;
            options.InstanceId = "itest";
        });
        builder.Services.Configure<CroniqTokenOptions>(builder.Configuration.GetSection("Croniq:Auth:Tokens"));
        builder.Services.AddCroniqApiRateLimiter();
        builder.Services.AddSingleton<TenantRateLimitDecider>();

        builder.WebHost.UseTestServer();

        builder.Services.AddSingleton<ICallerContextAccessor, CallerContextAccessor>();
        builder.Services.AddSingleton(CallerFactory);
        builder.Services.AddSingleton<ICallerContextFactory>(sp => sp.GetRequiredService<TestCallerContextFactory>());

        builder.Services.AddSingleton(Webhooks);
        builder.Services.AddSingleton<IWebhookPersistenceProvider>(sp => sp.GetRequiredService<InMemoryWebhookPersistenceProvider>());

        builder.Services.AddSingleton(DeadLetters);
        builder.Services.AddSingleton<IWebhookDeadLetterStore>(sp => sp.GetRequiredService<InMemoryWebhookDeadLetterStore>());

        builder.Services.AddSingleton(Pipeline);
        builder.Services.AddSingleton<IJobExecutionPipeline>(sp => sp.GetRequiredService<RecordingJobExecutionPipeline>());

        builder.Services.AddSingleton<IJobTrigger, DefaultJobTrigger>();

        builder.Services.AddSingleton(ExecutionLogs);
        builder.Services.AddSingleton<IExecutionLogReader>(sp => sp.GetRequiredService<TestExecutionLogReader>());

        builder.Services.AddSingleton(ExecutionHistory);
        builder.Services.AddSingleton<IExecutionHistoryReader>(sp => sp.GetRequiredService<TestExecutionHistoryReader>());

        builder.Services.AddSingleton(JobDeadLetters);
        builder.Services.AddSingleton<IJobDeadLetterStore>(sp => sp.GetRequiredService<InMemoryJobDeadLetterStore>());

        builder.Services.AddSingleton(Registry);
        builder.Services.AddSingleton<IJobRegistry>(sp => sp.GetRequiredService<FakeJobRegistry>());

        builder.Services.AddSingleton(Policies);
        builder.Services.AddSingleton<IPolicyResolver>(sp => sp.GetRequiredService<FakePolicyResolver>());

        builder.Services.AddSingleton(JobStore);
        builder.Services.AddSingleton<IJobPersistenceProvider>(sp => sp.GetRequiredService<NoopJobPersistenceProvider>());
        builder.Services.AddSingleton<IPersistenceHealth>(sp => sp.GetRequiredService<NoopJobPersistenceProvider>());

        builder.Services.AddSingleton(ApiKeys);
        builder.Services.AddSingleton<IApiKeyStore>(sp => sp.GetRequiredService<FakeApiKeyStore>());
        builder.Services.AddSingleton<ICroniqTokenIssuer, CroniqTokenIssuer>();

        builder.Services.AddSingleton(Tenants);
        builder.Services.AddSingleton<ITenantStore>(sp => sp.GetRequiredService<TestTenantStore>());

        _app = builder.Build();
        _app.UseCroniqApi();

        await _app.StartAsync().ConfigureAwait(false);
        Client = _app.GetTestClient();
        Client.DefaultRequestHeaders.Add("X-Croniq-Key", TestCallerContextFactory.ApiKey);
    }

    public async Task DisposeAsync()
    {
        Client?.Dispose();

        if (_app is null)
        {
            return;
        }

        await _app.StopAsync().ConfigureAwait(false);
        await _app.DisposeAsync().ConfigureAwait(false);
    }

    public void Reset()
    {
        CallerFactory.Reset();
        Webhooks.Clear();
        DeadLetters.Clear();
        Pipeline.Clear();
        ExecutionLogs.Clear();
        ExecutionHistory.Clear();
        JobDeadLetters.Clear();
        Registry.Clear();
        Policies.Reset();
        JobStore.Reset();
        ApiKeys.Reset();
        Tenants.Reset();

        if (Client is not null)
        {
            Client.DefaultRequestHeaders.Remove("X-Croniq-Key");
            Client.DefaultRequestHeaders.Add("X-Croniq-Key", TestCallerContextFactory.ApiKey);
        }
    }

    public JobDescriptor EnsureJob(string jobKey) => Registry.EnsureJob(jobKey);
}
