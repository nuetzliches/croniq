using System.Collections.Generic;
using System.Net.Http;
using Croniq.Api;
using Croniq.Api.Security;
using Croniq.Auth.Abstractions;
using Croniq.Auth.Core;
using Croniq.Auth.SqlServer;
using Croniq.Core.Execution;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks.InMemory;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Xunit;

namespace Croniq.Api.Tests.Infrastructure;

public sealed class PasswordAuthApiTestHost : IAsyncLifetime
{
    private WebApplication? _app;

    // Must match the default tenant seeded by TestTenantStore.Reset().
    public const string TenantId = TestCallerContextFactory.DefaultTenantId;
    public const string Environment = "dev";

    public HttpClient Client { get; private set; } = default!;

    public IServiceProvider Services => _app?.Services ?? throw new InvalidOperationException("Host not started yet");

    public InMemoryPasswordUserStore Users { get; } = new();

    public InMemoryRefreshTokenStore RefreshTokens { get; } = new();

    public InMemoryWebhookPersistenceProvider Webhooks { get; } = new();

    public InMemoryWebhookDeadLetterStore DeadLetters { get; } = new();

    public RecordingJobExecutionPipeline Pipeline { get; } = new();

    public TestExecutionLogReader ExecutionLogs { get; } = new();

    public TestExecutionHistoryReader ExecutionHistory { get; } = new();

    public FakeJobRegistry Registry { get; } = new();

    public FakePolicyResolver Policies { get; } = new();

    public NoopJobPersistenceProvider JobStore { get; } = new();

    public InMemoryApiKeyStore ApiKeys { get; } = new(new[]
    {
        new ApiKeySeed(
            KeyId: "ak_test",
            Secret: "secret",
            TenantId,
            EnvironmentTag: Environment,
            Scopes: new[] { CroniqScopes.TenantsAdmin })
    });

    public TestTenantStore Tenants { get; } = new();

    public async Task InitializeAsync()
    {
        var builder = WebApplication.CreateBuilder(new WebApplicationOptions
        {
            ApplicationName = typeof(PasswordAuthApiTestHost).Assembly.FullName,
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
            ["Croniq:Auth:Tokens:SigningKey"] = "Y3JvbmlxLWl0ZXN0LXNpZ25pbmcta2V5LTEyMzQ1Njc4OTA=",
            ["Croniq:Auth:Password:Enabled"] = "true",
            ["Croniq:Auth:Password:DefaultTenant"] = TenantId,
            ["Croniq:Auth:Password:MaxFailedAccessAttempts"] = "2",
            ["Croniq:Auth:Password:LockoutMinutes"] = "15",
            ["Croniq:Auth:Password:AccessTokenLifetimeMinutes"] = "15",
            ["Croniq:Auth:Password:RefreshTokenLifetimeDays"] = "7"
        });

        builder.Services.AddLogging();
        builder.Services.AddRouting();
        builder.Services.AddOptions();

        builder.Services.Configure<CroniqApiOptions>(builder.Configuration.GetSection("Croniq:Api"));
        builder.Services.Configure<CroniqOidcOptions>(builder.Configuration.GetSection("Croniq:Auth:Oidc"));
        builder.Services.Configure<CroniqTokenOptions>(builder.Configuration.GetSection("Croniq:Auth:Tokens"));
        builder.Services.Configure<PasswordAuthOptions>(builder.Configuration.GetSection("Croniq:Auth:Password"));

        builder.Services.AddCroniqApiRateLimiter();
        builder.Services.AddSingleton<TenantRateLimitDecider>();

        builder.WebHost.UseTestServer();

        builder.Services.AddSingleton<ICallerContextAccessor, CallerContextAccessor>();
        builder.Services.AddSingleton<IApiKeyStore>(ApiKeys);
        builder.Services.AddSingleton<ICroniqTokenIssuer, CroniqTokenIssuer>();

        builder.Services.AddSingleton<ICallerContextFactory, CallerContextFactory>();

        builder.Services.AddSingleton(Users);
        builder.Services.AddSingleton<IPasswordUserStore>(sp => sp.GetRequiredService<InMemoryPasswordUserStore>());

        builder.Services.AddSingleton(RefreshTokens);
        builder.Services.AddSingleton<IRefreshTokenStore>(sp => sp.GetRequiredService<InMemoryRefreshTokenStore>());

        builder.Services.AddSingleton<PasswordAuthService>();

        builder.Services.AddSingleton(Webhooks);
        builder.Services.AddSingleton<IWebhookPersistenceProvider>(sp => sp.GetRequiredService<InMemoryWebhookPersistenceProvider>());

        builder.Services.AddSingleton(DeadLetters);
        builder.Services.AddSingleton<IWebhookDeadLetterStore>(sp => sp.GetRequiredService<InMemoryWebhookDeadLetterStore>());

        builder.Services.AddSingleton(Pipeline);
        builder.Services.AddSingleton<IJobExecutionPipeline>(sp => sp.GetRequiredService<RecordingJobExecutionPipeline>());

        builder.Services.AddSingleton(ExecutionLogs);
        builder.Services.AddSingleton<IExecutionLogReader>(sp => sp.GetRequiredService<TestExecutionLogReader>());

        builder.Services.AddSingleton(ExecutionHistory);
        builder.Services.AddSingleton<IExecutionHistoryReader>(sp => sp.GetRequiredService<TestExecutionHistoryReader>());

        builder.Services.AddSingleton(Registry);
        builder.Services.AddSingleton<IJobRegistry>(sp => sp.GetRequiredService<FakeJobRegistry>());

        builder.Services.AddSingleton(Policies);
        builder.Services.AddSingleton<IPolicyResolver>(sp => sp.GetRequiredService<FakePolicyResolver>());

        builder.Services.AddSingleton(JobStore);
        builder.Services.AddSingleton<IJobPersistenceProvider>(sp => sp.GetRequiredService<NoopJobPersistenceProvider>());
        builder.Services.AddSingleton<IPersistenceHealth>(sp => sp.GetRequiredService<NoopJobPersistenceProvider>());

        builder.Services.AddSingleton(Tenants);
        builder.Services.AddSingleton<ITenantStore>(sp => sp.GetRequiredService<TestTenantStore>());

        _app = builder.Build();
        _app.UseCroniqApi();

        await _app.StartAsync().ConfigureAwait(false);
        Client = _app.GetTestClient();
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
}
