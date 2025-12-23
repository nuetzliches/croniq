using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Net;
using System.Net.Http.Json;
using Croniq.Api.Models;
using Croniq.Api.Tests.Infrastructure;
using Croniq.Persistence.Abstractions;
using Shouldly;
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
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var body = await response.Content.ReadFromJsonAsync<List<WebhookEndpointResponse>>();
        body.ShouldNotBeNull();
        body!.Count.ShouldBe(1);
        var endpoint = body[0];
        endpoint.HookKey.ShouldBe("hook-order-created");
        endpoint.Metadata.ShouldNotBeNull();
        endpoint.Metadata!.ContainsKey("source").ShouldBeTrue();
        endpoint.IpRules.ShouldNotBeNull();
        endpoint.IpRules!.ShouldBeEmpty();
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

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var body = await response.Content.ReadFromJsonAsync<WebhookEndpointResponse>();
        body.ShouldNotBeNull();
        body!.Secret.ShouldBe("whsec_custom");
        body.RequireSignature.ShouldBeFalse();
        body.Metadata.ShouldNotBeNull();
        body.Metadata!.ContainsKey("team").ShouldBeTrue();

        var persisted = _host.Webhooks.Find("hook-invoices", _host.DefaultScope);
        persisted.ShouldNotBeNull();
        persisted!.RequireSignature.ShouldBeFalse();
        persisted.Secret.ShouldBe("whsec_custom");
    }

    [Fact]
    public async Task DeleteWebhookRemovesDefinition()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "shipment-dispatched");
        _host.Webhooks.Seed("hook-shipment", jobKey, _host.DefaultScope, secret: "whsec_to_delete");

        var response = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/hook-shipment?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.NoContent);
        _host.Webhooks.Find("hook-shipment", _host.DefaultScope).ShouldBeNull();
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

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var payload = await response.Content.ReadFromJsonAsync<RotateWebhookSecretResponse>();
        payload.ShouldNotBeNull();
        payload!.Secret.ShouldNotBe(seeded.Secret);
        payload.ExpiresAtUtc.ShouldNotBeNull();

        var current = _host.Webhooks.Find(seeded.HookKey, _host.DefaultScope);
        current.ShouldNotBeNull();
        current!.Secret.ShouldBe(payload.Secret);
    }

    [Fact]
    public async Task WebhookIpRuleCrudFlowWorks()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "ip-allow");
        _host.Webhooks.Seed("hook-ip-guard", jobKey, _host.DefaultScope, secret: "whsec_guard");

        var createRequest = new CreateWebhookIpRuleRequest("10.10.0.0/24", "corp");
        var createResponse = await _host.Client.PostAsJsonAsync(
            $"/tenants/{WebhookApiTestHost.TenantId}/webhooks/hook-ip-guard/ip-rules?environment={WebhookApiTestHost.Environment}",
            createRequest);

        createResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var created = await createResponse.Content.ReadFromJsonAsync<WebhookIpRuleResponse>();
        created.ShouldNotBeNull();
        created!.Cidr.ShouldBe("10.10.0.0/24");
        created.Description.ShouldBe("corp");

        var listResponse = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/hook-ip-guard/ip-rules?environment={WebhookApiTestHost.Environment}");
        listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
        var rules = await listResponse.Content.ReadFromJsonAsync<List<WebhookIpRuleResponse>>();
        rules.ShouldNotBeNull();
        var singleRule = rules!.ShouldHaveSingleItem();
        singleRule.Id.ShouldBe(created.Id);
        singleRule.Cidr.ShouldBe("10.10.0.0/24");

        var deleteResponse = await _host.Client.DeleteAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/hook-ip-guard/ip-rules/{created.Id}?environment={WebhookApiTestHost.Environment}");
        deleteResponse.StatusCode.ShouldBe(HttpStatusCode.NoContent);

        var finalList = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/hook-ip-guard/ip-rules?environment={WebhookApiTestHost.Environment}");
        finalList.StatusCode.ShouldBe(HttpStatusCode.OK);
        var empty = await finalList.Content.ReadFromJsonAsync<List<WebhookIpRuleResponse>>();
        empty.ShouldNotBeNull();
        empty!.ShouldBeEmpty();
    }

    [Fact]
    public async Task DeadLetterListingReturnsEntries()
    {
        _host.Reset();
        var jobKey = BuildJobKey("ops", "customer-sync");
        var entry = _host.DeadLetters.Seed("hook-sync", jobKey, _host.DefaultScope, payload: "{\"status\":\"failed\"}", failureReason: "timeout");

        var response = await _host.Client.GetAsync($"/tenants/{WebhookApiTestHost.TenantId}/webhooks/deadletters?environment={WebhookApiTestHost.Environment}");
        response.StatusCode.ShouldBe(HttpStatusCode.OK);

        var items = await response.Content.ReadFromJsonAsync<List<WebhookDeadLetterResponse>>();
        items.ShouldNotBeNull();
        items!.Count.ShouldBe(1);
        var single = items.Single();
        single.Id.ShouldBe(entry.Id);
        single.FailureReason.ShouldBe("timeout");
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

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var execution = _host.Pipeline.Executions.ShouldHaveSingleItem();
        execution.JobKey.Value.ShouldBe(jobKey);
        execution.Metadata.ShouldNotBeNull();
        execution.Metadata!.ContainsKey("webhook:deadletter:id").ShouldBeTrue();
        execution.Metadata["webhook:deadletter:id"].ShouldBe(entry.Id.ToString(CultureInfo.InvariantCulture));

        _host.DeadLetters.Contains(entry.Id).ShouldBeFalse();
    }

    private static string BuildJobKey(string namespaceSegment, string jobName)
    {
        return $"{namespaceSegment}:{jobName}";
    }
}
