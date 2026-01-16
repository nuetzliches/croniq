export interface TenantScopedParams {
    tenantId: string;
}

export interface TenantEnvironmentOptionalParams extends TenantScopedParams {
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

export type TenantUpsertApiClientParams = TenantEnvironmentOptionalParams;

export type TenantApiClientTokenParams = TenantApiClientParams;

export interface TenantWebhookParams extends TenantEnvironmentParams {
    hookKey: string;
}

export type TenantWebhookUpsertParams = TenantEnvironmentParams;

export type TenantWebhookCapabilitiesParams = TenantEnvironmentParams;

export interface TenantWebhookRuleParams extends TenantWebhookParams {
    ruleId: string;
}

export interface TenantDeadLetterParams extends TenantEnvironmentParams {
    deadLetterId: number;
}

export interface WebhookInvocationParams {
    hookKey: string;
}

export interface ExecutionLogParams extends TenantScopedParams {
    executionId: string;
}

export interface WorkEventsParams extends ExecutionLogParams {
    environment?: string | null;
}

export interface ExecutionParams extends TenantEnvironmentOptionalParams {
    jobKey?: string | null;
    status?: number | null;
    executionId?: string | null;
    startedAfterUtc?: string | null;
    startedBeforeUtc?: string | null;
    limit?: number | null;
}

export interface TenantScheduleParams extends TenantEnvironmentParams {
    triggerId: string;
}

export interface DashboardForecastParams extends TenantEnvironmentOptionalParams {
    windowMinutes?: number | null;
    bucketMinutes?: number | null;
    summaryMinutes?: string | null;
}

export interface CallerContext {
    actor?: string;
    tenantId?: string;
    environment?: string;
    source?: string;
    command?: string;
}

export interface CroniqCredentialSupplier {
    getSessionToken(): string | null;
}

export interface CroniqRequestOptions {
    context?: CallerContext;
    sessionToken?: string | null;
}
