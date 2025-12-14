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
import { EndpointExecutor, requireEndpoint } from './endpoint-executor';

const LIST_SCHEDULES_ENDPOINT: EndpointDefinition = {
    method: 'get',
    path: '/schedules',
    requestFormat: 'json',
    response: scheduleListResponseSchema,
};

const UPSERT_SCHEDULE_ENDPOINT = requireEndpoint(SchedulesApi, 'post', '/schedules');
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
const EXECUTION_LOG_ENDPOINT = requireEndpoint(ExecutionsApi, 'get', '/executions/:executionId/logs');

const HEALTH_ENDPOINTS = {
    service: requireEndpoint(HealthApi, 'get', '/health'),
    persistence: requireEndpoint(HealthApi, 'get', '/health/persistence'),
};


export interface CroniqApiClient {
    getSchedules(options?: CroniqRequestOptions): Promise<ScheduleListResponse>;
    upsertSchedule(payload: UpsertScheduleRequest, options?: CroniqRequestOptions): Promise<void>;
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

class HttpCroniqApiClient implements CroniqApiClient {
    private readonly executor: EndpointExecutor;

    constructor(http: HttpClient, baseUrl: string) {
        this.executor = new EndpointExecutor(http, baseUrl);
    }

    getSchedules(options?: CroniqRequestOptions): Promise<ScheduleListResponse> {
        return this.executor.execute<ScheduleListResponse>(LIST_SCHEDULES_ENDPOINT, {
            context: options?.context,
            responseSchema: scheduleListResponseSchema,
        });
    }

    upsertSchedule(payload: UpsertScheduleRequest, options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(UPSERT_SCHEDULE_ENDPOINT, {
            body: payload,
            context: options?.context,
        });
    }

    triggerJob(payload: TriggerJobRequest, options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(TRIGGER_JOB_ENDPOINT, {
            body: payload,
            context: options?.context,
        });
    }

    issueTenantApiKey(
        params: TenantScopedParams,
        payload: IssueApiKeyRequest,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.executor.execute(TENANT_ENDPOINTS.issueApiKey, {
            path: { tenantId: params.tenantId },
            body: payload,
            context: options?.context,
        });
    }

    rotateTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.rotateApiKey, {
            path: {
                tenantId: params.tenantId,
                keyId: params.keyId,
            },
            query: {
                environment: params.environment ?? undefined,
            },
            context: options?.context,
        });
    }

    deleteTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.deleteApiKey, {
            path: {
                tenantId: params.tenantId,
                keyId: params.keyId,
            },
            query: {
                environment: params.environment ?? undefined,
            },
            context: options?.context,
        });
    }

    getTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Promise<unknown> {
        return this.executor.execute(TENANT_ENDPOINTS.apiClient, {
            path: {
                tenantId: params.tenantId,
                clientId: params.clientId,
            },
            query: {
                environment: params.environment ?? undefined,
            },
            context: options?.context,
        });
    }

    listTenantWebhooks(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.executor.execute(TENANT_ENDPOINTS.listWebhooks, {
            path: { tenantId: params.tenantId },
            query: { environment: params.environment },
            context: options?.context,
        });
    }

    upsertTenantWebhook(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.upsertWebhook, {
            path: { tenantId: params.tenantId },
            query: {
                environment: params.environment,
                allowUnsigned: params.allowUnsigned,
            },
            body: payload,
            context: options?.context,
        });
    }

    deleteTenantWebhook(params: TenantWebhookParams, options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.deleteWebhook, {
            path: {
                tenantId: params.tenantId,
                hookKey: params.hookKey,
            },
            query: { environment: params.environment },
            context: options?.context,
        });
    }

    rotateTenantWebhookSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.rotateWebhookSecret, {
            path: {
                tenantId: params.tenantId,
                hookKey: params.hookKey,
            },
            query: { environment: params.environment },
            body: payload,
            context: options?.context,
        });
    }

    listTenantWebhookIpRules(
        params: TenantWebhookParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.executor.execute(TENANT_ENDPOINTS.listIpRules, {
            path: {
                tenantId: params.tenantId,
                hookKey: params.hookKey,
            },
            query: { environment: params.environment },
            context: options?.context,
        });
    }

    createTenantWebhookIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.createIpRule, {
            path: {
                tenantId: params.tenantId,
                hookKey: params.hookKey,
            },
            query: { environment: params.environment },
            body: payload,
            context: options?.context,
        });
    }

    deleteTenantWebhookIpRule(
        params: TenantWebhookRuleParams,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.deleteIpRule, {
            path: {
                tenantId: params.tenantId,
                hookKey: params.hookKey,
                ruleId: params.ruleId,
            },
            query: { environment: params.environment },
            context: options?.context,
        });
    }

    listTenantWebhookDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        return this.executor.execute(TENANT_ENDPOINTS.listDeadLetters, {
            path: { tenantId: params.tenantId },
            query: { environment: params.environment },
            context: options?.context,
        });
    }

    replayTenantWebhookDeadLetter(
        params: TenantDeadLetterParams,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        return this.executor.execute(TENANT_ENDPOINTS.replayDeadLetter, {
            path: {
                tenantId: params.tenantId,
                deadLetterId: params.deadLetterId,
            },
            query: { environment: params.environment },
            context: options?.context,
        });
    }

    invokeWebhook(params: WebhookInvocationParams, options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(INVOKE_WEBHOOK_ENDPOINT, {
            path: { hookKey: params.hookKey },
            context: options?.context,
        });
    }

    fetchExecutionLogs(params: ExecutionLogParams, options?: CroniqRequestOptions): Promise<string> {
        return this.executor.execute<string>(EXECUTION_LOG_ENDPOINT, {
            path: { executionId: params.executionId },
            context: options?.context,
            responseType: 'text',
            parseResponse: false,
        });
    }

    checkServiceHealth(options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(HEALTH_ENDPOINTS.service, {
            context: options?.context,
        });
    }

    checkPersistenceHealth(options?: CroniqRequestOptions): Promise<void> {
        return this.executor.execute(HEALTH_ENDPOINTS.persistence, {
            context: options?.context,
        });
    }
}

export const CRONIQ_API_CLIENT = new InjectionToken<CroniqApiClient>('CRONIQ_API_CLIENT', {
    providedIn: 'root',
    factory: () => new HttpCroniqApiClient(inject(HttpClient), inject(CRONIQ_API_BASE_URL)),
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
