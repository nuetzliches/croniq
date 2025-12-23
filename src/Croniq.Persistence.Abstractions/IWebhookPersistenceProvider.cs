using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public interface IWebhookPersistenceProvider
{
    Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListAsync(PartitionScope scope, CancellationToken cancellationToken);

    Task UpsertAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken);

    Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken);

    Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken);

    Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken);

    Task DeleteIpRuleAsync(long ruleId, PartitionScope scope, string? deletedBy, string? correlationId, CancellationToken cancellationToken);
}
