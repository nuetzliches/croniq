using System;
using System.Collections.Generic;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Webhooks;
using Croniq.Webhooks.InMemory;
using Croniq.Webhooks.Options;
using Croniq.Webhooks.Remote;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public class WebhookHostingExtensionsTests
{
    [Fact]
    public void AddCroniqWebhookPersistence_UsesInMemory_ByDefault()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Webhooks:Mode"] = "InMemory"
            })
            .Build();

        services.AddCroniqWebhookPersistence(config);
        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<IWebhookPersistenceProvider>()
            .ShouldBeOfType<InMemoryWebhookPersistenceProvider>();
        provider.GetRequiredService<IWebhookDeadLetterStore>()
            .ShouldBeOfType<InMemoryWebhookDeadLetterStore>();
    }

    [Fact]
    public void AddCroniqWebhookPersistence_UsesSqlServer_WhenConfigured()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Webhooks:Mode"] = "SqlServer",
                ["Croniq:Webhooks:SqlServer:ConnectionString"] = "Server=localhost;Database=Croniq;Trusted_Connection=True;Encrypt=False"
            })
            .Build();

        services.AddCroniqWebhookPersistence(config);
        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<IWebhookPersistenceProvider>()
            .ShouldBeOfType<SqlServerWebhookPersistenceProvider>();
        provider.GetRequiredService<IWebhookDeadLetterStore>()
            .ShouldBeOfType<SqlServerWebhookDeadLetterStore>();
    }

    [Fact]
    public void AddCroniqWebhookPersistence_UsesRemote_WhenConfigured()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Webhooks:Mode"] = "Remote",
                ["Croniq:Webhooks:Remote:BaseUrl"] = "https://dmz.croniq.test/api",
                ["Croniq:Webhooks:Remote:ApiKey"] = "dmz-key"
            })
            .Build();

        services.AddCroniqWebhookPersistence(config);
        var provider = services.BuildServiceProvider();

        provider.GetRequiredService<IWebhookPersistenceProvider>()
            .ShouldBeOfType<RemoteWebhookPersistenceProvider>();
        provider.GetRequiredService<IWebhookDeadLetterStore>()
            .ShouldBeOfType<RemoteWebhookDeadLetterStore>();
    }

    [Fact]
    public void AddCroniqWebhookPersistence_Throws_WhenRemoteMissingBaseUrl()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Webhooks:Mode"] = "Remote",
                ["Croniq:Webhooks:Remote:ApiKey"] = "dmz-key"
            })
            .Build();

        Should.Throw<InvalidOperationException>(() => services.AddCroniqWebhookPersistence(config));
    }

    [Fact]
    public void AddCroniqWebhookServices_Throws_WhenUnsignedHooksDisallowed()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Auth:Mode"] = "InMemory",
                ["Croniq:Webhooks:Endpoints:0:HookKey"] = "hook-1",
                ["Croniq:Webhooks:Endpoints:0:JobKey"] = "ns:job",
                ["Croniq:Webhooks:Endpoints:0:RequireSignature"] = "false",
                ["Croniq:Webhooks:Security:AllowUnsignedHooks"] = "false"
            })
            .Build();

        services.AddCroniqWebhookServices(config);
        var provider = services.BuildServiceProvider();

        Should.Throw<InvalidOperationException>(() =>
            provider.GetRequiredService<IOptions<CroniqWebhookOptions>>().Value);
    }

    [Fact]
    public void AddCroniqWebhookServices_AllowsUnsignedHooks_WhenEnabled()
    {
        var services = new ServiceCollection();
        var config = new ConfigurationBuilder()
            .AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Auth:Mode"] = "InMemory",
                ["Croniq:Webhooks:Endpoints:0:HookKey"] = "hook-1",
                ["Croniq:Webhooks:Endpoints:0:JobKey"] = "ns:job",
                ["Croniq:Webhooks:Endpoints:0:RequireSignature"] = "false",
                ["Croniq:Webhooks:Security:AllowUnsignedHooks"] = "true"
            })
            .Build();

        services.AddCroniqWebhookServices(config);
        var provider = services.BuildServiceProvider();
        var options = provider.GetRequiredService<IOptions<CroniqWebhookOptions>>().Value;

        options.Security.AllowUnsignedHooks.ShouldBeTrue();
        options.Endpoints.ShouldNotBeEmpty();
        options.Endpoints[0].RequireSignature.ShouldBeFalse();
    }
}
