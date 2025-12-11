namespace Croniq.Sdk.Operator.Webhooks;

public sealed record WebhookIpRule(
    long Id,
    string Cidr,
    string? Description,
    string? CreatedBy,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc);

public sealed record WebhookIpRuleCreateRequest(string Cidr, string? Description = null);

public sealed record WebhookIpRuleDesired(string Cidr, string? Description = null);

public sealed record WebhookIpRuleSyncResult(
    IReadOnlyList<WebhookIpRule> Created,
    IReadOnlyList<long> DeletedRuleIds,
    IReadOnlyList<WebhookIpRule> FinalState);
