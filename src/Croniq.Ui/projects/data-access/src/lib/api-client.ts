import { HttpClient } from '@angular/common/http';
import { EnvironmentProviders, InjectionToken, Provider, inject, makeEnvironmentProviders } from '@angular/core';
import { AuthApi, CalendarResponse, CalendarResponseLooseSchema, CalendarUpsertResult, CreateWebhookIpRuleRequest, CroniqCalendarSeedDefinition, HealthApi, IssueApiKeyRequest, IssueTokenRequest, JobsApi, MeApi, PasswordChangePasswordRequest, PasswordLoginRequest, PasswordLogoutRequest, PasswordRefreshRequest, RotateWebhookSecretRequest, RunnerHeartbeatRequest, RunnerListResponse, ScheduleDeadLetterResponse, ScheduleForecastResponse, ScheduleResponse, ScheduleUpsertResult, TenantsApi, TriggerJobRequest, UpsertApiClientRequest, UpsertJobRequest, UpsertScheduleRequest, UpsertTenantRequest, UpsertWebhookEndpointRequest, WebhookActivitySummary, WebhookActivityTimelineResponse, WebhookCapabilitiesResponse, WorkerHeartbeatRequest, WorkerListResponse, WorkAckRequest, WorkEventsRequest, WorkPollRequest, WorkRenewRequest, type EndpointDefinition } from '@croniq/api-schema';
import type { Observable } from 'rxjs';
import { z } from 'zod';
import type { CroniqCredentialSupplier, CroniqRequestOptions, DashboardForecastParams, ExecutionLogParams, ExecutionParams, TenantApiClientParams, TenantApiClientTokenParams, TenantApiKeyParams, TenantCalendarParams, TenantDeadLetterParams, TenantEnvironmentOptionalParams, TenantEnvironmentParams, TenantScheduleParams, TenantScopedParams, TenantUpsertApiClientParams, TenantWebhookActivityParams, TenantWebhookActivitySummaryParams, TenantWebhookCapabilitiesParams, TenantWebhookParams, TenantWebhookRuleParams, TenantWebhookUpsertParams, WebhookInvocationParams, WorkEventsParams } from './api-client.types';
import type { EndpointCallConfig } from './endpoint-executor';
import { EndpointExecutor, requireEndpoint } from './endpoint-executor';

const TENANT_LIST_SCHEDULES_ENDPOINT = requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/schedules');
const LIST_SCHEDULES_ENDPOINT: EndpointDefinition = {
    ...TENANT_LIST_SCHEDULES_ENDPOINT,
    response: z.array(ScheduleResponse),
};

const UPSERT_SCHEDULE_ENDPOINT = requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/schedules');
const UPSERT_SCHEDULE_WITH_RESPONSE_ENDPOINT: EndpointDefinition = {
    ...UPSERT_SCHEDULE_ENDPOINT,
    response: ScheduleUpsertResult,
};

const TRIGGER_JOB_ENDPOINT = requireEndpoint(JobsApi, 'post', '/jobs/trigger');

const ME_ENDPOINT = requireEndpoint(MeApi, 'get', '/me');

const AUTH_ENDPOINTS = {
    login: requireEndpoint(AuthApi, 'post', '/auth/login'),
    refresh: requireEndpoint(AuthApi, 'post', '/auth/refresh'),
    logout: requireEndpoint(AuthApi, 'post', '/auth/logout'),
    changePassword: requireEndpoint(AuthApi, 'post', '/auth/change-password'),
};

const TENANTS_ENDPOINTS = {
    list: requireEndpoint(TenantsApi, 'get', '/tenants'),
    create: requireEndpoint(TenantsApi, 'post', '/tenants'),
    get: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId'),
    deactivate: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId'),
};

const LIST_WORKERS_ENDPOINT_PATH = '/tenants/:tenantId/workers';
const LIST_WORKERS_ENDPOINT: EndpointDefinition =
    TenantsApi.find((entry) => entry.method === 'get' && entry.path === LIST_WORKERS_ENDPOINT_PATH) ?? {
        method: 'get',
        path: LIST_WORKERS_ENDPOINT_PATH,
        requestFormat: 'json',
        parameters: [
            { name: 'tenantId', type: 'Path', schema: z.string() },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: WorkerListResponse,
    };

const WORKER_HEARTBEAT_ENDPOINT_PATH = '/tenants/:tenantId/workers/heartbeat';
const WORKER_HEARTBEAT_ENDPOINT: EndpointDefinition =
    TenantsApi.find((entry) => entry.method === 'post' && entry.path === WORKER_HEARTBEAT_ENDPOINT_PATH) ?? {
        method: 'post',
        path: WORKER_HEARTBEAT_ENDPOINT_PATH,
        requestFormat: 'json',
        parameters: [
            { name: 'body', type: 'Body', schema: WorkerHeartbeatRequest },
            { name: 'tenantId', type: 'Path', schema: z.string() },
            {
                name: 'environment',
                type: 'Query',
                schema: z.string().optional(),
            },
        ],
        response: z.void(),
    };

const TENANT_ENDPOINTS = {
    apiClient: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/api-clients/:clientId'),
    listApiClients: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/api-clients'),
    upsertApiClient: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/api-clients'),
    deleteApiClient: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId/api-clients/:clientId'),
    issueApiClientToken: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/api-clients/:clientId/tokens'),
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
    listExecutions: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/executions'),
    getExecution: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/executions/:executionId'),
    listJobs: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/jobs'),
    upsertJob: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/jobs'),
    getJob: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/jobs/:jobId'),
    deleteJob: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId/jobs/:jobId'),
    listCalendars: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/calendars'),
    upsertCalendar: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/calendars'),
    getCalendar: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/calendars/:calendarId'),
    deleteCalendar: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId/calendars/:calendarId'),
    getSchedule: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/schedules/:triggerId'),
    deleteSchedule: requireEndpoint(TenantsApi, 'delete', '/tenants/:tenantId/schedules/:triggerId'),
    dashboardForecast: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/dashboard/forecast'),
    listScheduleDeadLetters: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/schedules/deadletters'),
    replayScheduleDeadLetter: requireEndpoint(
        TenantsApi,
        'post',
        '/tenants/:tenantId/schedules/deadletters/:deadLetterId/replay',
    ),
    listRunners: requireEndpoint(TenantsApi, 'get', '/tenants/:tenantId/runners'),
    runnerHeartbeat: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/runners/heartbeat'),
    listWorkers: LIST_WORKERS_ENDPOINT,
    workerHeartbeat: WORKER_HEARTBEAT_ENDPOINT,
    issueToken: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/tokens'),
    workEvents: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/work/:executionId:events'),
    workAck: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/work/ack'),
    workPoll: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/work/poll'),
    workRenew: requireEndpoint(TenantsApi, 'post', '/tenants/:tenantId/work/renew'),
};

const WEBHOOK_CAPABILITIES_ENDPOINT = requireEndpoint(
    TenantsApi,
    'get',
    '/tenants/:tenantId/webhooks/capabilities',
);

const WEBHOOK_ACTIVITY_TIMELINE_ENDPOINT_PATH = '/tenants/:tenantId/webhooks/activity';
const WEBHOOK_ACTIVITY_TIMELINE_ENDPOINT: EndpointDefinition = {
    ...(TenantsApi.find((entry) => entry.method === 'get' && entry.path === WEBHOOK_ACTIVITY_TIMELINE_ENDPOINT_PATH) ?? {
        method: 'get',
        path: WEBHOOK_ACTIVITY_TIMELINE_ENDPOINT_PATH,
        requestFormat: 'json',
        parameters: [
            { name: 'tenantId', type: 'Path', schema: z.string() },
            { name: 'environment', type: 'Query', schema: z.string().optional() },
            { name: 'fromUtc', type: 'Query', schema: z.string().optional() },
            { name: 'toUtc', type: 'Query', schema: z.string().optional() },
            { name: 'hookKeys', type: 'Query', schema: z.string().optional() },
            { name: 'jobKeys', type: 'Query', schema: z.string().optional() },
            { name: 'limit', type: 'Query', schema: z.number().int().optional() },
        ],
    }),
    response: WebhookActivityTimelineResponse,
};

const WEBHOOK_ACTIVITY_SUMMARY_ENDPOINT_PATH = '/tenants/:tenantId/webhooks/activity/summary';
const WEBHOOK_ACTIVITY_SUMMARY_ENDPOINT: EndpointDefinition =
    TenantsApi.find((entry) => entry.method === 'get' && entry.path === WEBHOOK_ACTIVITY_SUMMARY_ENDPOINT_PATH) ?? {
        method: 'get',
        path: WEBHOOK_ACTIVITY_SUMMARY_ENDPOINT_PATH,
        requestFormat: 'json',
        parameters: [
            { name: 'tenantId', type: 'Path', schema: z.string() },
            { name: 'environment', type: 'Query', schema: z.string().optional() },
            { name: 'fromUtc', type: 'Query', schema: z.string().optional() },
            { name: 'toUtc', type: 'Query', schema: z.string().optional() },
            { name: 'hookKeys', type: 'Query', schema: z.string().optional() },
            { name: 'jobKeys', type: 'Query', schema: z.string().optional() },
            { name: 'bucketMinutes', type: 'Query', schema: z.number().int().optional() },
        ],
        response: WebhookActivitySummary,
    };

const INVOKE_WEBHOOK_ENDPOINT_PATH =
    '/tenants/:tenantId/environments/:environmentTag/webhooks/:hookKey/invoke';
const INVOKE_WEBHOOK_ENDPOINT: EndpointDefinition =
    TenantsApi.find((entry) => entry.method === 'post' && entry.path === INVOKE_WEBHOOK_ENDPOINT_PATH) ?? {
        method: 'post',
        path: INVOKE_WEBHOOK_ENDPOINT_PATH,
        requestFormat: 'json',
        parameters: [
            { name: 'tenantId', type: 'Path', schema: z.string() },
            { name: 'environmentTag', type: 'Path', schema: z.string() },
            { name: 'hookKey', type: 'Path', schema: z.string() },
        ],
        response: z.void(),
    };
const EXECUTION_LOG_ENDPOINT = requireEndpoint(
    TenantsApi,
    'get',
    '/tenants/:tenantId/executions/:executionId/logs',
);

const HEALTH_ENDPOINTS = {
    service: requireEndpoint(HealthApi, 'get', '/health'),
    persistence: requireEndpoint(HealthApi, 'get', '/health/persistence'),
};

const joinCsv = (values?: ReadonlyArray<string> | null): string | undefined => {
    if (!values || values.length === 0) {
        return undefined;
    }
    return Array.from(new Set(values)).join(',');
};


export interface CroniqApiClient {
    passwordLogin(payload: PasswordLoginRequest, options?: CroniqRequestOptions): Observable<unknown>;
    passwordRefresh(payload: PasswordRefreshRequest, options?: CroniqRequestOptions): Observable<unknown>;
    passwordLogout(payload: PasswordLogoutRequest, options?: CroniqRequestOptions): Observable<void>;
    passwordChangePassword(payload: PasswordChangePasswordRequest, options?: CroniqRequestOptions): Observable<void>;

    getSchedules(
        params: TenantScopedParams & { environment?: string | null; jobKey?: string | null },
        options?: CroniqRequestOptions,
    ): Observable<ScheduleResponse[]>;
    upsertSchedule(
        params: TenantEnvironmentParams,
        payload: UpsertScheduleRequest,
        options?: CroniqRequestOptions,
    ): Observable<ScheduleUpsertResult>;
    listCalendars(
        params: TenantScopedParams & { environment?: string | null },
        options?: CroniqRequestOptions,
    ): Observable<CalendarResponse[]>;
    upsertCalendar(
        params: TenantEnvironmentParams,
        payload: CroniqCalendarSeedDefinition,
        options?: CroniqRequestOptions,
    ): Observable<CalendarUpsertResult>;
    getCalendar(params: TenantCalendarParams, options?: CroniqRequestOptions): Observable<CalendarResponse>;
    deleteCalendar(params: TenantCalendarParams, options?: CroniqRequestOptions): Observable<void>;
    triggerJob(payload: TriggerJobRequest, options?: CroniqRequestOptions): Observable<void>;

    getMe(options?: CroniqRequestOptions): Observable<unknown>;

    listTenants(params?: { state?: string | null }, options?: CroniqRequestOptions): Observable<unknown>;
    createTenant(payload: UpsertTenantRequest, options?: CroniqRequestOptions): Observable<void>;
    getTenant(params: TenantScopedParams, options?: CroniqRequestOptions): Observable<unknown>;
    deactivateTenant(params: TenantScopedParams, options?: CroniqRequestOptions): Observable<void>;

    issueApiClientToken(
        params: TenantApiClientTokenParams,
        payload: IssueTokenRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;

    listTenantApiClients(params: TenantUpsertApiClientParams, options?: CroniqRequestOptions): Observable<unknown>;
    upsertTenantApiClient(
        params: TenantUpsertApiClientParams,
        payload: UpsertApiClientRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    deleteTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Observable<void>;
    issueTenantApiKey(
        params: TenantScopedParams,
        payload: IssueApiKeyRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    rotateTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Observable<void>;
    deleteTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Observable<void>;
    getTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Observable<unknown>;

    listTenantWebhooks(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    getTenantWebhookCapabilities(
        params: TenantWebhookCapabilitiesParams,
        options?: CroniqRequestOptions,
    ): Observable<WebhookCapabilitiesResponse>;
    upsertTenantWebhook(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    deleteTenantWebhook(params: TenantWebhookParams, options?: CroniqRequestOptions): Observable<void>;
    rotateTenantWebhookSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    listTenantWebhookIpRules(
        params: TenantWebhookParams,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    createTenantWebhookIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    deleteTenantWebhookIpRule(params: TenantWebhookRuleParams, options?: CroniqRequestOptions): Observable<void>;
    listTenantWebhookDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    replayTenantWebhookDeadLetter(params: TenantDeadLetterParams, options?: CroniqRequestOptions): Observable<void>;
    listTenantWebhookActivity(
        params: TenantWebhookActivityParams,
        options?: CroniqRequestOptions,
    ): Observable<WebhookActivityTimelineResponse>;
    getTenantWebhookActivitySummary(
        params: TenantWebhookActivitySummaryParams,
        options?: CroniqRequestOptions,
    ): Observable<WebhookActivitySummary>;
    invokeWebhook(params: WebhookInvocationParams, options?: CroniqRequestOptions): Observable<void>;

    listExecutions(params: ExecutionParams, options?: CroniqRequestOptions): Observable<unknown>;
    getExecution(params: ExecutionParams & ExecutionLogParams, options?: CroniqRequestOptions): Observable<unknown>;
    listJobs(params: TenantEnvironmentParams, options?: CroniqRequestOptions): Observable<unknown>;
    upsertJob(params: TenantEnvironmentParams, payload: UpsertJobRequest, options?: CroniqRequestOptions): Observable<void>;
    getJob(params: TenantEnvironmentParams & { jobId: string }, options?: CroniqRequestOptions): Observable<unknown>;
    deleteJob(params: TenantEnvironmentParams & { jobId: string }, options?: CroniqRequestOptions): Observable<void>;
    getSchedule(params: TenantScheduleParams, options?: CroniqRequestOptions): Observable<unknown>;
    deleteSchedule(params: TenantScheduleParams, options?: CroniqRequestOptions): Observable<void>;
    getScheduleForecast(
        params: DashboardForecastParams,
        options?: CroniqRequestOptions,
    ): Observable<ScheduleForecastResponse>;

    listTenantScheduleDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Observable<ScheduleDeadLetterResponse[]>;
    replayTenantScheduleDeadLetter(params: TenantDeadLetterParams, options?: CroniqRequestOptions): Observable<void>;

    listRunners(params: TenantEnvironmentOptionalParams, options?: CroniqRequestOptions): Observable<RunnerListResponse>;
    runnerHeartbeat(
        params: TenantEnvironmentOptionalParams,
        payload: RunnerHeartbeatRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    listWorkers(params: TenantEnvironmentOptionalParams, options?: CroniqRequestOptions): Observable<WorkerListResponse>;
    workerHeartbeat(
        params: TenantEnvironmentOptionalParams,
        payload: WorkerHeartbeatRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    issueToken(
        params: TenantEnvironmentOptionalParams,
        payload: IssueTokenRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    workEvents(
        params: WorkEventsParams,
        payload: WorkEventsRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    workAck(
        params: TenantEnvironmentOptionalParams,
        payload: WorkAckRequest,
        options?: CroniqRequestOptions,
    ): Observable<void>;
    workPoll(
        params: TenantEnvironmentOptionalParams,
        payload: WorkPollRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;
    workRenew(
        params: TenantEnvironmentOptionalParams,
        payload: WorkRenewRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown>;

    fetchExecutionLogs(params: ExecutionLogParams, options?: CroniqRequestOptions): Observable<string>;
    checkServiceHealth(options?: CroniqRequestOptions): Observable<void>;
    checkPersistenceHealth(options?: CroniqRequestOptions): Observable<void>;
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

    getSchedules(
        params: TenantScopedParams & { environment?: string | null; jobKey?: string | null },
        options?: CroniqRequestOptions,
    ): Observable<ScheduleResponse[]> {
        return this.execute$<ScheduleResponse[]>(
            LIST_SCHEDULES_ENDPOINT,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment ?? options?.context?.environment ?? undefined,
                    jobKey: params.jobKey ?? undefined,
                },
                responseSchema: z.array(ScheduleResponse),
            },
            options,
        );
    }

    upsertSchedule(
        params: TenantEnvironmentParams,
        payload: UpsertScheduleRequest,
        options?: CroniqRequestOptions,
    ): Observable<ScheduleUpsertResult> {
        return this.execute$<ScheduleUpsertResult>(
            UPSERT_SCHEDULE_WITH_RESPONSE_ENDPOINT,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
                body: payload,
                responseSchema: ScheduleUpsertResult,
            },
            options,
        );
    }

    listCalendars(
        params: TenantScopedParams & { environment?: string | null },
        options?: CroniqRequestOptions,
    ): Observable<CalendarResponse[]> {
        return this.execute$<CalendarResponse[]>(
            TENANT_ENDPOINTS.listCalendars,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment ?? options?.context?.environment ?? undefined,
                },
                responseSchema: z.array(CalendarResponseLooseSchema),
            },
            options,
        );
    }

    upsertCalendar(
        params: TenantEnvironmentParams,
        payload: CroniqCalendarSeedDefinition,
        options?: CroniqRequestOptions,
    ): Observable<CalendarUpsertResult> {
        return this.execute$<CalendarUpsertResult>(
            TENANT_ENDPOINTS.upsertCalendar,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
                body: payload,
                responseSchema: CalendarUpsertResult,
            },
            options,
        );
    }

    getCalendar(params: TenantCalendarParams, options?: CroniqRequestOptions): Observable<CalendarResponse> {
        return this.execute$<CalendarResponse>(
            TENANT_ENDPOINTS.getCalendar,
            {
                path: {
                    tenantId: params.tenantId,
                    calendarId: params.calendarId,
                },
                query: { environment: params.environment },
                responseSchema: CalendarResponseLooseSchema,
            },
            options,
        );
    }

    deleteCalendar(params: TenantCalendarParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.deleteCalendar,
            {
                path: {
                    tenantId: params.tenantId,
                    calendarId: params.calendarId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    triggerJob(payload: TriggerJobRequest, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TRIGGER_JOB_ENDPOINT,
            {
                body: payload,
            },
            options,
        );
    }

    passwordLogin(payload: PasswordLoginRequest, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            AUTH_ENDPOINTS.login,
            {
                body: payload,
            },
            options,
        );
    }

    passwordRefresh(payload: PasswordRefreshRequest, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            AUTH_ENDPOINTS.refresh,
            {
                body: payload,
            },
            options,
        );
    }

    passwordLogout(payload: PasswordLogoutRequest, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            AUTH_ENDPOINTS.logout,
            {
                body: payload,
            },
            options,
        );
    }

    passwordChangePassword(payload: PasswordChangePasswordRequest, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            AUTH_ENDPOINTS.changePassword,
            {
                body: payload,
            },
            options,
        );
    }

    getMe(options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(ME_ENDPOINT, {}, options);
    }

    listTenants(params?: { state?: string | null }, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANTS_ENDPOINTS.list,
            {
                query: {
                    state: params?.state ?? undefined,
                },
            },
            options,
        );
    }

    createTenant(payload: UpsertTenantRequest, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANTS_ENDPOINTS.create,
            {
                body: payload,
            },
            options,
        );
    }

    getTenant(params: TenantScopedParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANTS_ENDPOINTS.get,
            {
                path: { tenantId: params.tenantId },
            },
            options,
        );
    }

    deactivateTenant(params: TenantScopedParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANTS_ENDPOINTS.deactivate,
            {
                path: { tenantId: params.tenantId },
            },
            options,
        );
    }

    issueApiClientToken(
        params: TenantApiClientTokenParams,
        payload: IssueTokenRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.issueApiClientToken,
            {
                path: {
                    tenantId: params.tenantId,
                    clientId: params.clientId,
                },
                query: {
                    environment: params.environment ?? undefined,
                },
                body: payload,
            },
            options,
        );
    }

    listTenantApiClients(params: TenantUpsertApiClientParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.listApiClients,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment ?? undefined,
                },
            },
            options,
        );
    }

    upsertTenantApiClient(
        params: TenantUpsertApiClientParams,
        payload: UpsertApiClientRequest,
        options?: CroniqRequestOptions,
    ): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.upsertApiClient,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment ?? undefined,
                },
                body: payload,
            },
            options,
        );
    }

    deleteTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.deleteApiClient,
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

    issueTenantApiKey(
        params: TenantScopedParams,
        payload: IssueApiKeyRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.issueApiKey,
            {
                path: { tenantId: params.tenantId },
                body: payload,
            },
            options,
        );
    }

    rotateTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
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

    deleteTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
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

    getTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
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

    listExecutions(params: ExecutionParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.listExecutions,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment,
                    jobKey: params.jobKey ?? undefined,
                    status: params.status ?? undefined,
                    startedAfterUtc: params.startedAfterUtc ?? undefined,
                    startedBeforeUtc: params.startedBeforeUtc ?? undefined,
                    limit: params.limit ?? undefined,
                },
            },
            options,
        );
    }

    getExecution(params: ExecutionParams & ExecutionLogParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.getExecution,
            {
                path: {
                    tenantId: params.tenantId,
                    executionId: params.executionId,
                },
                query: {
                    environment: params.environment,
                },
            },
            options,
        );
    }

    listJobs(params: TenantEnvironmentParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.listJobs,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
            },
            options,
        );
    }

    upsertJob(params: TenantEnvironmentParams, payload: UpsertJobRequest, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.upsertJob,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
                body: payload,
            },
            options,
        );
    }

    getJob(params: TenantEnvironmentParams & { jobId: string }, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.getJob,
            {
                path: {
                    tenantId: params.tenantId,
                    jobId: params.jobId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    deleteJob(params: TenantEnvironmentParams & { jobId: string }, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.deleteJob,
            {
                path: {
                    tenantId: params.tenantId,
                    jobId: params.jobId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    getSchedule(params: TenantScheduleParams, options?: CroniqRequestOptions): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.getSchedule,
            {
                path: {
                    tenantId: params.tenantId,
                    triggerId: params.triggerId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    deleteSchedule(params: TenantScheduleParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.deleteSchedule,
            {
                path: {
                    tenantId: params.tenantId,
                    triggerId: params.triggerId,
                },
                query: { environment: params.environment },
            },
            options,
        );
    }

    getScheduleForecast(
        params: DashboardForecastParams,
        options?: CroniqRequestOptions,
    ): Observable<ScheduleForecastResponse> {
        return this.execute$<ScheduleForecastResponse>(
            TENANT_ENDPOINTS.dashboardForecast,
            {
                path: {
                    tenantId: params.tenantId,
                },
                query: {
                    environment: params.environment ?? options?.context?.environment ?? undefined,
                    windowMinutes: params.windowMinutes ?? undefined,
                    bucketMinutes: params.bucketMinutes ?? undefined,
                    summaryMinutes: params.summaryMinutes ?? undefined,
                },
            },
            options,
        );
    }

    listTenantScheduleDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Observable<ScheduleDeadLetterResponse[]> {
        return this.execute$<ScheduleDeadLetterResponse[]>(
            TENANT_ENDPOINTS.listScheduleDeadLetters,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
            },
            options,
        );
    }

    replayTenantScheduleDeadLetter(params: TenantDeadLetterParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.replayScheduleDeadLetter,
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

    listTenantWebhooks(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.listWebhooks,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment },
            },
            options,
        );
    }

    getTenantWebhookCapabilities(
        params: TenantWebhookCapabilitiesParams,
        options?: CroniqRequestOptions,
    ): Observable<WebhookCapabilitiesResponse> {
        return this.execute$<WebhookCapabilitiesResponse>(
            WEBHOOK_CAPABILITIES_ENDPOINT,
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
    ): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.upsertWebhook,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment,
                },
                body: payload,
            },
            options,
        );
    }

    deleteTenantWebhook(params: TenantWebhookParams, options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(
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
    ): Observable<unknown> {
        return this.execute$(
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
    ): Observable<unknown> {
        return this.execute$(
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
    ): Observable<void> {
        return this.execute$(
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
    ): Observable<void> {
        return this.execute$(
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
    ): Observable<unknown> {
        return this.execute$(
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
    ): Observable<void> {
        return this.execute$(
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

    listTenantWebhookActivity(
        params: TenantWebhookActivityParams,
        options?: CroniqRequestOptions,
    ): Observable<WebhookActivityTimelineResponse> {
        return this.execute$<WebhookActivityTimelineResponse>(
            WEBHOOK_ACTIVITY_TIMELINE_ENDPOINT,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment ?? undefined,
                    fromUtc: params.fromUtc ?? undefined,
                    toUtc: params.toUtc ?? undefined,
                    hookKeys: joinCsv(params.hookKeys),
                    jobKeys: joinCsv(params.jobKeys),
                    limit: params.limit ?? undefined,
                },
            },
            options,
        );
    }

    getTenantWebhookActivitySummary(
        params: TenantWebhookActivitySummaryParams,
        options?: CroniqRequestOptions,
    ): Observable<WebhookActivitySummary> {
        return this.execute$<WebhookActivitySummary>(
            WEBHOOK_ACTIVITY_SUMMARY_ENDPOINT,
            {
                path: { tenantId: params.tenantId },
                query: {
                    environment: params.environment ?? undefined,
                    fromUtc: params.fromUtc ?? undefined,
                    toUtc: params.toUtc ?? undefined,
                    hookKeys: joinCsv(params.hookKeys),
                    jobKeys: joinCsv(params.jobKeys),
                    bucketMinutes: params.bucketMinutes ?? undefined,
                },
            },
            options,
        );
    }

    listRunners(params: TenantEnvironmentOptionalParams, options?: CroniqRequestOptions): Observable<RunnerListResponse> {
        return this.execute$(
            TENANT_ENDPOINTS.listRunners,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
            },
            options,
        );
    }

    runnerHeartbeat(
        params: TenantEnvironmentOptionalParams,
        payload: RunnerHeartbeatRequest,
        options?: CroniqRequestOptions,
    ): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.runnerHeartbeat,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    listWorkers(params: TenantEnvironmentOptionalParams, options?: CroniqRequestOptions): Observable<WorkerListResponse> {
        return this.execute$(
            TENANT_ENDPOINTS.listWorkers,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
            },
            options,
        );
    }

    workerHeartbeat(
        params: TenantEnvironmentOptionalParams,
        payload: WorkerHeartbeatRequest,
        options?: CroniqRequestOptions,
    ): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.workerHeartbeat,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    issueToken(
        params: TenantEnvironmentOptionalParams,
        payload: IssueTokenRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.issueToken,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    workEvents(
        params: WorkEventsParams,
        payload: WorkEventsRequest,
        options?: CroniqRequestOptions,
    ): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.workEvents,
            {
                path: {
                    tenantId: params.tenantId,
                    executionId: params.executionId,
                },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    workAck(
        params: TenantEnvironmentOptionalParams,
        payload: WorkAckRequest,
        options?: CroniqRequestOptions,
    ): Observable<void> {
        return this.execute$(
            TENANT_ENDPOINTS.workAck,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    workPoll(
        params: TenantEnvironmentOptionalParams,
        payload: WorkPollRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.workPoll,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    workRenew(
        params: TenantEnvironmentOptionalParams,
        payload: WorkRenewRequest,
        options?: CroniqRequestOptions,
    ): Observable<unknown> {
        return this.execute$(
            TENANT_ENDPOINTS.workRenew,
            {
                path: { tenantId: params.tenantId },
                query: { environment: params.environment ?? undefined },
                body: payload,
            },
            options,
        );
    }

    invokeWebhook(params: WebhookInvocationParams, options?: CroniqRequestOptions): Observable<void> {
        const tenantId = options?.context?.tenantId?.trim() ?? '';
        const environmentTag = options?.context?.environment?.trim() ?? '';
        if (!tenantId || !environmentTag) {
            throw new Error('invokeWebhook requires request options with tenantId + environment in context.');
        }
        return this.execute$(
            INVOKE_WEBHOOK_ENDPOINT,
            {
                path: {
                    tenantId,
                    environmentTag,
                    hookKey: params.hookKey,
                },
            },
            options,
        );
    }

    fetchExecutionLogs(params: ExecutionLogParams, options?: CroniqRequestOptions): Observable<string> {
        return this.execute$<string>(
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

    checkServiceHealth(options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(HEALTH_ENDPOINTS.service, {}, options);
    }

    checkPersistenceHealth(options?: CroniqRequestOptions): Observable<void> {
        return this.execute$(HEALTH_ENDPOINTS.persistence, {}, options);
    }

    private execute$<T>(
        endpoint: EndpointDefinition,
        config: EndpointCallConfig,
        options?: CroniqRequestOptions,
    ): Observable<T> {
        return this.executor.execute$<T>(endpoint, this.withRequestOptions(config, options));
    }

    private withRequestOptions(config: EndpointCallConfig, options?: CroniqRequestOptions): EndpointCallConfig {
        if (!options) {
            return config;
        }
        const merged: EndpointCallConfig = { ...config };
        if (merged.context === undefined) {
            merged.context = options.context;
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

export type { CallerContext, CroniqCredentialSupplier, CroniqRequestOptions, DashboardForecastParams, ExecutionLogParams, TenantApiClientParams, TenantApiKeyParams, TenantCalendarParams, TenantDeadLetterParams, TenantEnvironmentParams, TenantScopedParams, TenantWebhookActivityParams, TenantWebhookActivitySummaryParams, TenantWebhookCapabilitiesParams, TenantWebhookParams, TenantWebhookRuleParams, TenantWebhookUpsertParams, WebhookActivityStatus, WebhookInvocationParams } from './api-client.types';
