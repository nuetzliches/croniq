using System.Collections.Generic;
using System.Globalization;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Persistence.Abstractions;
using FluentAssertions;
using Xunit;

namespace Croniq.Api.Tests;

public sealed class WebhookEndpointIntegrationTests : IClassFixture<WebhookApiTestHost>
{
    private readonly WebhookApiTestHost _host;

    public WebhookEndpointIntegrationTests(WebhookApiTestHost host)
    {
        _host = host;
    }

    [Fact]
    public async Task ListingWebhooksReturnsSeededEndpoints()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "order-created");
        _host.Webhooks.Seed("hook-order-created", jobKey, _host.DefaultScope, secret: "whsec_seeded", metadata: new Dictionary<string, string> { { "source", "tests" } });

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.Should().Be(HttpStatusCode.OK);

        var body = await response.Content.ReadFromJsonAsync<List<WebhookEndpointResponse>>();
        body.Should().NotBeNull().And.HaveCount(1);
        body![0].HookKey.Should().Be("hook-order-created");
        body[0].Metadata.Should().NotBeNull().And.ContainKey("source");
    }

    [Fact]
    public async Task UpsertingWebhookAllowsUnsignedFlowWhenFlagPresent()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "invoice-generated");

        var request = new UpsertWebhookEndpointRequest(
            HookKey: "hook-invoices",
            JobKey: jobKey,
            Enabled: true,
            RequireSignature: false,
            RequestsPerMinute: 30,
            Secret: "whsec_custom",
            Metadata: new Dictionary<string, string> { { "team", "billing" } },
            SignatureVersion: 2);

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/webhooks?environment={WebhookApiTestHost.Environment}&allowUnsigned=true",
            request);

        response.StatusCode.Should().Be(HttpStatusCode.OK);
        var body = await response.Content.ReadFromJsonAsync<WebhookEndpointResponse>();
        body.Should().NotBeNull();
        body!.Secret.Should().Be("whsec_custom");
        body.RequireSignature.Should().BeFalse();
        body.Metadata.Should().ContainKey("team");

        var persisted = _host.Webhooks.Find("hook-invoices");
        persisted.Should().NotBeNull();
        persisted!.RequireSignature.Should().BeFalse();
        persisted.Secret.Should().Be("whsec_custom");
    }

    [Fact]
    public async Task DeleteWebhookRemovesDefinition()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "shipment-dispatched");
        _host.Webhooks.Seed("hook-shipment", jobKey, _host.DefaultScope, secret: "whsec_to_delete");

        var response = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/hook-shipment?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.Should().Be(HttpStatusCode.NoContent);
        _host.Webhooks.Find("hook-shipment").Should().BeNull();
    }

    [Fact]
    public async Task RotateSecretUpdatesStoredMaterial()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "file-exported");
        var seeded = _host.Webhooks.Seed("hook-export", jobKey, _host.DefaultScope, secret: "whsec_old");

        var response = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/webhooks/{seeded.HookKey}/rotate-secret?environment={WebhookApiTestHost.Environment}",
            new RotateWebhookSecretRequest(ActivateInSeconds: 5, GracePeriodSeconds: 30, Notes: "itest"));

        response.StatusCode.Should().Be(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<RotateWebhookSecretResponse>();
        payload.Should().NotBeNull();
        payload!.Secret.Should().NotBe(seeded.Secret);
        payload.ExpiresAtUtc.Should().NotBeNull();

        var current = _host.Webhooks.Find(seeded.HookKey);
        current.Should().NotBeNull();
        current!.Secret.Should().Be(payload.Secret);
    }

    [Fact]
    public async Task DeadLetterListingReturnsEntries()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "customer-sync");
        var entry = _host.DeadLetters.Seed("hook-sync", jobKey, _host.DefaultScope, payload: "{\"status\":\"failed\"}", failureReason: "timeout");

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/deadletters?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.Should().Be(HttpStatusCode.OK);

        var items = await response.Content.ReadFromJsonAsync<List<WebhookDeadLetterResponse>>();
        items.Should().NotBeNull();
        items!.Should().ContainSingle(d => d.Id == entry.Id && d.FailureReason == "timeout");
    }

    [Fact]
    public async Task DeadLetterReplayDispatchesPipelineAndResolvesEntry()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "document-archive");
        _host.EnsureJob(jobKey);
        var entry = _host.DeadLetters.Seed("hook-archive", jobKey, _host.DefaultScope, payload: "{\"file\":\"abc\"}", failureReason: "handler-crash");

        var response = await _host.Client.PostAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/webhooks/deadletters/{entry.Id}/replay?environment={WebhookApiTestHost.Environment}",
            JsonContent.Create(new { }));

        response.StatusCode.Should().Be(HttpStatusCode.OK);
        var execution = _host.Pipeline.Executions.Should().ContainSingle().Subject;
        execution.JobKey.Value.Should().Be(jobKey);
        execution.Metadata.Should().NotBeNull();
        execution.Metadata!.Should().ContainKey("webhook:deadletter:id").WhoseValue.Should().Be(entry.Id.ToString(CultureInfo.InvariantCulture));

        _host.DeadLetters.Contains(entry.Id).Should().BeFalse();
    }

    private static string BuildJobKey(string namespaceSegment, string jobName)
    {
        return $"{WebhookApiTestHost.TenantId}:{WebhookApiTestHost.Environment}:{namespaceSegment}:{jobName}";
    }
}
