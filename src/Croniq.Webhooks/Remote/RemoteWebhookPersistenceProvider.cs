using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Persistence.Abstractions;

namespace Croniq.Webhooks.Remote;

public sealed class RemoteWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private readonly RemoteWebhookClient _client;

    public RemoteWebhookPersistenceProvider(RemoteWebhookClient client)
    {
        _client = client ?? throw new ArgumentNullException(nameof(client));
    }

    public async Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        var endpoints = await _client.ListEndpointsAsync(scope, cancellationToken).ConfigureAwait(false);
        return endpoints.FirstOrDefault(endpoint =>
            string.Equals(endpoint.HookKey, hookKey, StringComparison.OrdinalIgnoreCase));
    }

    public Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        return _client.ListEndpointsAsync(scope, cancellationToken);
    }

    public Task UpsertAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (!JobKey.TryParse(request.JobKey, out _))
        {
            throw new InvalidOperationException($"JobKey '{request.JobKey}' is invalid.");
        }

        return _client.UpsertEndpointAsync(request, cancellationToken);
    }

    public Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        return _client.DeleteEndpointAsync(hookKey, scope, hardDelete, cancellationToken);
    }

    public Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        return _client.RotateSecretAsync(request, cancellationToken);
    }

    public Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        throw new InvalidOperationException("Remote webhook persistence does not expose secrets. Use SqlServer mode for ingress.");
    }

    public Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        return _client.ListIpRulesAsync(hookKey, scope, cancellationToken);
    }

    public Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        return _client.AddIpRuleAsync(request, cancellationToken);
    }

    public async Task DeleteIpRuleAsync(long ruleId, PartitionScope scope, string? deletedBy, string? correlationId, CancellationToken cancellationToken)
    {
        var endpoints = await _client.ListEndpointsAsync(scope, cancellationToken).ConfigureAwait(false);
        foreach (var endpoint in endpoints)
        {
            if (endpoint.IpRules.Any(rule => rule.Id == ruleId))
            {
                await _client.DeleteIpRuleAsync(endpoint.HookKey, ruleId, scope, cancellationToken).ConfigureAwait(false);
                return;
            }
        }
    }
}
