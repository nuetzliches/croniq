using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Api.Models;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Croniq.Sdk;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.TestHost;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Shouldly;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WorkAckPolicyTests
{
    private const string RunnerId = "default";

    [Fact]
    public async Task AckFailure_WithoutNextFireTime_CreatesDeadLetter()
    {
        await using var host = await WorkApiTestHost.CreateAsync();
        const string jobKey = "ops:work-deadletter";

        await SeedDueTriggerAsync(host.JobStore, host.Scope, jobKey);

        var lease = await PollSingleLeaseAsync(host.Client, host.TenantId, host.EnvironmentTag);
        var ack = new WorkAckRequest(
            EnvironmentTag: host.EnvironmentTag,
            RunnerId: host.RunnerId,
            Lease: lease,
            Succeeded: false,
            NextFireTimeUtc: null,
            DeadLetterReason: null);

        var ackResponse = await host.Client.PostAsJsonAsync($"/tenants/{host.TenantId}/work/ack", ack);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var deadLetters = await host.DeadLetters.ListAsync(host.Scope, CancellationToken.None);
        deadLetters.ShouldHaveSingleItem().Reason.ShouldBe("work-failed");
    }

    [Fact]
    public async Task AckFailure_WithNextFireTime_DoesNotCreateDeadLetter()
    {
        await using var host = await WorkApiTestHost.CreateAsync();
        const string jobKey = "ops:work-retry";

        await SeedDueTriggerAsync(host.JobStore, host.Scope, jobKey);

        var lease = await PollSingleLeaseAsync(host.Client, host.TenantId, host.EnvironmentTag);
        var retryAt = DateTimeOffset.UtcNow.AddMinutes(2);
        var ack = new WorkAckRequest(
            EnvironmentTag: host.EnvironmentTag,
            RunnerId: host.RunnerId,
            Lease: lease,
            Succeeded: false,
            NextFireTimeUtc: retryAt,
            DeadLetterReason: "retry");

        var ackResponse = await host.Client.PostAsJsonAsync($"/tenants/{host.TenantId}/work/ack", ack);
        ackResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var deadLetters = await host.DeadLetters.ListAsync(host.Scope, CancellationToken.None);
        deadLetters.ShouldBeEmpty();

        var triggers = await host.JobStore.ListTriggersAsync(host.Scope, CancellationToken.None);
        triggers.Count.ShouldBe(1);
        var trigger = triggers.Single();
        trigger.StartAtUtc.ShouldNotBeNull();
        var startAt = trigger.StartAtUtc.GetValueOrDefault();
        startAt.ShouldBeInRange(retryAt.AddSeconds(-1), retryAt.AddSeconds(1));
    }

    private static async Task SeedDueTriggerAsync(IJobPersistenceProvider store, PartitionScope scope, string jobKey)
    {
        await store.UpsertJobAsync(
            new JobDefinition(jobKey, "ops", "work", Variant: null, Description: null, Metadata: null, AssignedRunnerId: RunnerId),
            scope,
            CancellationToken.None);

        var triggerId = $"{jobKey}:once-{Guid.NewGuid():N}";
        var trigger = new TriggerDefinition(
            triggerId,
            jobKey,
            TriggerSchedule.OnceExpression,
            scope,
            StartAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            EndAtUtc: null,
            Enabled: true,
            Metadata: null,
            TimeZoneId: TimeZoneInfo.Utc.Id);

        await store.UpsertTriggerAsync(trigger, CancellationToken.None);
    }

    private static async Task<WorkLeaseToken> PollSingleLeaseAsync(HttpClient client, string tenantId, string environmentTag)
    {
        var poll = new WorkPollRequest(
            EnvironmentTag: environmentTag,
            RunnerId: RunnerId,
            BatchSize: 1);

        var response = await client.PostAsJsonAsync($"/tenants/{tenantId}/work/poll", poll);
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var payload = await response.Content.ReadFromJsonAsync<WorkPollResponse>();
        payload.ShouldNotBeNull();
        payload.Leases.Length.ShouldBe(1);
        return payload.Leases[0];
    }

    private sealed class WorkApiTestHost : IAsyncDisposable
    {
        private WorkApiTestHost(
            WebApplication app,
            HttpClient client,
            IJobPersistenceProvider jobStore,
            IJobDeadLetterStore deadLetters,
            PartitionScope scope,
            string tenantId,
            string environmentTag,
            string runnerId)
        {
            App = app;
            Client = client;
            JobStore = jobStore;
            DeadLetters = deadLetters;
            Scope = scope;
            TenantId = tenantId;
            EnvironmentTag = environmentTag;
            RunnerId = runnerId;
        }

        public WebApplication App { get; }
        public HttpClient Client { get; }
        public IJobPersistenceProvider JobStore { get; }
        public IJobDeadLetterStore DeadLetters { get; }
        public PartitionScope Scope { get; }
        public string TenantId { get; }
        public string EnvironmentTag { get; }
        public string RunnerId { get; }

        public static async Task<WorkApiTestHost> CreateAsync()
        {
            var apiKey = "ak_work_policy";
            var tenantId = "00000000-0000-0000-0000-000000000008";
            var environmentTag = "dev";

            var builder = WebApplication.CreateBuilder(new WebApplicationOptions
            {
                ApplicationName = typeof(WorkAckPolicyTests).Assembly.FullName,
                EnvironmentName = Environments.Development
            });

            builder.Configuration.AddInMemoryCollection(new Dictionary<string, string?>
            {
                ["Croniq:Api:RequestsPerMinute"] = "0",
                ["Croniq:Auth:Mode"] = "InMemory",
                ["Croniq:Auth:InMemory:ApiKey"] = apiKey,
                ["Croniq:Auth:InMemory:TenantId"] = tenantId,
                ["Croniq:Auth:InMemory:EnvironmentTag"] = environmentTag
            });

            builder.Services.AddCroniqApiServices(builder.Configuration);
            builder.Services.AddCroniqApiRateLimiter();
            builder.Services.AddLogging();

            builder.WebHost.UseTestServer();

            var app = builder.Build();
            app.UseCroniqApi();
            await app.StartAsync().ConfigureAwait(false);

            var client = app.GetTestClient();
            client.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

            var jobStore = app.Services.GetRequiredService<IJobPersistenceProvider>();
            var deadLetters = app.Services.GetRequiredService<IJobDeadLetterStore>();
            var scope = new PartitionScope(tenantId, environmentTag);

            return new WorkApiTestHost(app, client, jobStore, deadLetters, scope, tenantId, environmentTag, WorkAckPolicyTests.RunnerId);
        }

        public async ValueTask DisposeAsync()
        {
            Client.Dispose();
            await App.StopAsync().ConfigureAwait(false);
            await App.DisposeAsync().ConfigureAwait(false);
        }
    }
}
