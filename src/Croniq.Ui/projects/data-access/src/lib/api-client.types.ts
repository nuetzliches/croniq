export interface TenantScopedParams {
    tenantId: string;
}

interface TenantEnvironmentOptionalParams extends TenantScopedParams {
    environment?: string | null;
}

export interface TenantEnvironmentParams extends TenantScopedParams {
    environment: string;
}

export interface TenantApiKeyParams extends TenantEnvironmentOptionalParams {
    keyId: string;
}

export interface TenantApiClientParams extends TenantEnvironmentOptionalParams {
    clientId: string;
}

export interface TenantWebhookParams extends TenantEnvironmentParams {
    hookKey: string;
}

export interface TenantWebhookUpsertParams extends TenantWebhookParams {
    allowUnsigned: boolean;
}

export interface TenantWebhookRuleParams extends TenantWebhookParams {
    ruleId: string;
}

export interface TenantDeadLetterParams extends TenantEnvironmentParams {
    deadLetterId: string;
}

export interface WebhookInvocationParams {
    hookKey: string;
}

export interface ExecutionLogParams {
    executionId: string;
}

export interface CallerContext {
    actor?: string;
    tenantId?: string;
    environment?: string;
    source?: string;
    command?: string;
}

export interface CroniqRequestOptions {
    context?: CallerContext;
}
