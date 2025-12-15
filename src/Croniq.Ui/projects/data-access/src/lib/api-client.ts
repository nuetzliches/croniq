import { HttpClient } from '@angular/common/http';
import { EnvironmentProviders, InjectionToken, Provider, inject, makeEnvironmentProviders } from '@angular/core';

import {
    CreateWebhookIpRuleRequest,
    ExecutionsApi,
    HealthApi,
    IssueApiKeyRequest,
    JobsApi,
    RotateWebhookSecretRequest,
    ScheduleListResponse,
    SchedulesApi,
    TenantsApi,
    TriggerJobRequest,
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
    WebhooksApi,
    scheduleListResponseSchema,
    type EndpointDefinition,
} from '@croniq/api-schema';

import type {
    CroniqCredentialSupplier,
    CroniqRequestOptions,
    ExecutionLogParams,
    TenantApiClientParams,
    TenantApiKeyParams,
    TenantDeadLetterParams,
    TenantEnvironmentParams,
    TenantScopedParams,
    TenantWebhookParams,
    TenantWebhookRuleParams,
    TenantWebhookUpsertParams,
    WebhookInvocationParams,
} from './api-client.types';
import type { EndpointCallConfig } from './endpoint-executor';
import { EndpointExecutor, requireEndpoint } from './endpoint-executor';

const LIST_SCHEDULES_ENDPOINT: EndpointDefinition = {
    method: 'get',
    path: '/tenants/:tenantId/schedules',
    requestFormat: 'json',
    response: scheduleListResponseSchema,
};

const UPSERT_SCHEDULE_ENDPOINT = requireEndpoint(SchedulesApi, 'post', '/tenants/:tenantId/schedules');
const TRIGGER_JOB_ENDPOINT = requireEndpoint(JobsApi, 'post', '/jobs/trigger');

const TENANT_ENDPOINTS = {
    apiClient: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/api-clients/:clientId'),
    issueApiKey: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/api-keys'),
    deleteApiKey: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId/api-keys/:keyId'),
    rotateApiKey: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/api-keys/:keyId/rotate'),
    listWebhooks: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/webhooks'),
    upsertWebhook: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/webhooks'),
    deleteWebhook: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId/webhooks/:hookKey'),
    listIpRules: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/webhooks/:hookKey/ip-rules'),
    createIpRule: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/webhooks/:hookKey/ip-rules'),
    deleteIpRule: requireEndpoint(
        TenantsApi,
        'delete',
        '/tenants/:tenantId/webhooks/:hookKey/ip-rules/:ruleId',
    ),
    rotateWebhookSecret: requireEndpoint(
        TenantsApi,
        'post',
        '/tenants/:tenantId/webhooks/:hookKey/rotate-secret',
    ),
    listDeadLetters: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/webhooks/deadletters'),
    replayDeadLetter: requireEndpoint(
        TenantsApi,
        'post',
        '/tenants/:tenantId/webhooks/deadletters/:deadLetterId/replay',
    ),
};

const INVOKE_WEBHOOK_ENDPOINT = requireEndpoint(WebhooksApi, 'post', '/webhooks/:hookKey');
const EXECUTION_LOG_ENDPOINT = requireEndpoint(
    ExecutionsApi,
    'get',
    '/tenants/:tenantId/executions/:executionId/logs',
);

const HEALTH_ENDPOINTS = {
    service: requireEndpoint(HealthApi, 'get', '/health'),
    persistence: requireEndpoint(HealthApi, 'get', '/health/persistence'),
};


export interface CroniqApiClient {
    getSchedules(params: TenantScopedParams, options?: CroniqRequestOptions): Promise<ScheduleListResponse>;
    upsertSchedule(
        params: TenantScopedParams,
        payload: UpsertScheduleRequest,
        options?: CroniqRequestOptions,
    ): Promise<void>;
    triggerJob(payload: TriggerJobRequest, options?: CroniqRequestOptions): Promise<void>;
    issueTenantApiKey(
        params: TenantScopedParams,
        payload: IssueApiKeyRequest,
        options?: CroniqRequestOptions,
    ): Promise<unknown>;
    rotateTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void>;
    deleteTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void>;
    getTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Promise<unknown>;
    listTenantWebhooks(params: TenantEnvironmentParams, options?: CroniqRequestOptions): Promise<unknown>;
    upsertTenantWebhook(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
        options?: CroniqRequestOptions,
    ): Promise<void>;
    deleteTenantWebhook(params: TenantWebhookParams, options?: CroniqRequestOptions): Promise<void>;
    rotateTenantWebhookSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
        options?: CroniqRequestOptions,
    ): Promise<void>;
    listTenantWebhookIpRules(params: TenantWebhookParams, options?: CroniqRequestOptions): Promise<unknown>;
    createTenantWebhookIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
        options?: CroniqRequestOptions,
    ): Promise<void>;
    deleteTenantWebhookIpRule(params: TenantWebhookRuleParams, options?: CroniqRequestOptions): Promise<void>;
    listTenantWebhookDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown>;
    replayTenantWebhookDeadLetter(params: TenantDeadLetterParams, options?: CroniqRequestOptions): Promise<void>;
    invokeWebhook(params: WebhookInvocationParams, options?: CroniqRequestOptions): Promise<void>;
    fetchExecutionLogs(params: ExecutionLogParams, options?: CroniqRequestOptions): Promise<string>;
    checkServiceHealth(options?: CroniqRequestOptions): Promise<void>;
    checkPersistenceHealth(options?: CroniqRequestOptions): Promise<void>;
}

export const CRONIQ_API_BASE_URL = new InjectionToken<string>('CRONIQ_API_BASE_URL', {
    providedIn: 'root',
    factory: () => 'https://api.croniq.dev',
});

export const CRONIQ_CREDENTIAL_SUPPLIER = new InjectionToken<CroniqCredentialSupplier | null>(
    'CRONIQ_CREDENTIAL_SUPPLIER',
    {
        providedIn: 'root',
        factory: () => null,
    },
);

class HttpCroniqApiClient implements CroniqApiClient {
    private readonly executor: EndpointExecutor;

    constructor(http: HttpClient, baseUrl: string, credentials?: CroniqCredentialSupplier | null) {
        this.executor = new EndpointExecutor(http, baseUrl, 'Croniq.Ui', credentials);
    }

    getSchedules(params: TenantScopedParams, options?: CroniqRequestOptions): Promise<ScheduleListResponse> {
        return this.execute<ScheduleListResponse>(
            LIST_SCHEDULES_ENDPOINT,
            {
                path: { tenantId: params.tenantId },
                responseSchema: scheduleListResponseSchema,
            },
            options,
        );
    }

    upsertSchedule(
        params: TenantScopedParams,
        payload: UpsertScheduleRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.execute(
            UPSERT_SCHEDULE_ENDPOINT,
            {
                path: { tenantId: params.tenantId },
                body: payload,
            },
            options,
        );
    }

    triggerJob(payload: TriggerJobRequest, options?: CroniqRequestOptions): Promise<void> {
        return this.execute(
            TRIGGER_JOB_ENDPOINT,
            {
                body: payload,
            },
            options,
        );
    }

    issueTenantApiKey(
        params: TenantScopedParams,
        payload: IssueApiKeyRequest,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.execute(
            TENANT_ENDPOINTS.issueApiKey,
            {
                path: { tenantId: params.tenantId },
                body: payload,
            },
            options,
        );
    }

    rotateTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.rotateApiKey,
            {
                path: {
                    tenantId: params.tenantId,
                    keyId: params.keyId,
                },
                query: {
                    environment: params.environment ?? undefined,
                },
            },
            options,
        );
    }

    deleteTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.deleteApiKey,
            {
                path: {
                    tenantId: params.tenantId,
                    keyId: params.keyId,
                },
                query: {
                    environment: params.environment ?? undefined,
                },
            },
            options,
        );
    }

    getTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Promise<unknown> {
        return this.execute(
            TENANT_ENDPOINTS.apiClient,
            {
                path: {
                    tenantId: params.tenantId,
                    clientId: params.clientId,
                },
                query: {
                    environment: params.environment ?? undefined,
                },
            },
            options,
        );
    }

    listTenantWebhooks(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.execute(
            TENANT_ENDPOINTS.listWebhooks,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
            },
            options,
        );
    }

    upsertTenantWebhook(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.upsertWebhook,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment,
                    allowUnsigned: params.allowUnsigned,
                },
                body: payload,
            },
            options,
        );
    }

    deleteTenantWebhook(params: TenantWebhookParams, options?: CroniqRequestOptions): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.deleteWebhook,
            {
                path: {
                    tenantId: params.tenantId,
                    hookKey: params.hookKey,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    rotateTenantWebhookSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.rotateWebhookSecret,
            {
                path: {
                    tenantId: params.tenantId,
                    hookKey: params.hookKey,
                },
                query: { environment: params.environment },
                body: payload,
            },
            options,
        );
    }

    listTenantWebhookIpRules(
        params: TenantWebhookParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.execute(
            TENANT_ENDPOINTS.listIpRules,
            {
                path: {
                    tenantId: params.tenantId,
                    hookKey: params.hookKey,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    createTenantWebhookIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.createIpRule,
            {
                path: {
                    tenantId: params.tenantId,
                    hookKey: params.hookKey,
                },
                query: { environment: params.environment },
                body: payload,
            },
            options,
        );
    }

    deleteTenantWebhookIpRule(
        params: TenantWebhookRuleParams,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.deleteIpRule,
            {
                path: {
                    tenantId: params.tenantId,
                    hookKey: params.hookKey,
                    ruleId: params.ruleId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    listTenantWebhookDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.execute(
            TENANT_ENDPOINTS.listDeadLetters,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
            },
            options,
        );
    }

    replayTenantWebhookDeadLetter(
        params: TenantDeadLetterParams,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.execute(
            TENANT_ENDPOINTS.replayDeadLetter,
            {
                path: {
                    tenantId: params.tenantId,
                    deadLetterId: params.deadLetterId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    invokeWebhook(params: WebhookInvocationParams, options?: CroniqRequestOptions): Promise<void> {
        return this.execute(
            INVOKE_WEBHOOK_ENDPOINT,
            {
                path: { hookKey: params.hookKey },
            },
            options,
        );
    }

    fetchExecutionLogs(params: ExecutionLogParams, options?: CroniqRequestOptions): Promise<string> {
        return this.execute<string>(
            EXECUTION_LOG_ENDPOINT,
            {
                path: {
                    tenantId: params.tenantId,
                    executionId: params.executionId,
                },
                responseType: 'text',
                parseResponse: false,
            },
            options,
        );
    }

    checkServiceHealth(options?: CroniqRequestOptions): Promise<void> {
        return this.execute(HEALTH_ENDPOINTS.service, {}, options);
    }

    checkPersistenceHealth(options?: CroniqRequestOptions): Promise<void> {
        return this.execute(HEALTH_ENDPOINTS.persistence, {}, options);
    }

    private execute<T>(
        endpoint: EndpointDefinition,
        config: EndpointCallConfig,
        options?: CroniqRequestOptions,
    ): Promise<T> {
        return this.executor.execute<T>(endpoint, this.withRequestOptions(config, options));
    }

    private withRequestOptions(config: EndpointCallConfig, options?: CroniqRequestOptions): EndpointCallConfig {
        if (!options) {
            return config;
        }
        const merged: EndpointCallConfig = { ...config };
        if (merged.context === undefined) {
            merged.context = options.context;
        }
        if (!('apiKey' in merged)) {
            merged.apiKey = options.apiKey ?? null;
        }
        if (!('sessionToken' in merged)) {
            merged.sessionToken = options.sessionToken ?? null;
        }
        return merged;
    }
}

export const CRONIQ_API_CLIENT = new InjectionToken<CroniqApiClient>('CRONIQ_API_CLIENT', {
    providedIn: 'root',
    factory: () =>
        new HttpCroniqApiClient(
            inject(HttpClient),
            inject(CRONIQ_API_BASE_URL),
            inject(CRONIQ_CREDENTIAL_SUPPLIER),
        ),
});

export function provideCroniqApiClient(config: { baseUrl?: string } = {}): EnvironmentProviders {
    const providers: Provider[] = [];
    if (config.baseUrl) {
        providers.push({ provide: CRONIQ_API_BASE_URL, useValue: config.baseUrl });
    }
    return makeEnvironmentProviders(providers);
}

export type {
    CallerContext,
    CroniqCredentialSupplier,
    CroniqRequestOptions,
    ExecutionLogParams,
    TenantApiClientParams,
    TenantApiKeyParams,
    TenantDeadLetterParams,
    TenantEnvironmentParams,
    TenantScopedParams,
    TenantWebhookParams,
    TenantWebhookRuleParams,
    TenantWebhookUpsertParams,
    WebhookInvocationParams
} from './api-client.types';

