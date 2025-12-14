import { HttpClient } from '@angular/common/http';
import { EnvironmentProviders, InjectionToken, Provider, inject, makeEnvironmentProviders } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { map } from 'rxjs/operators';

import {
    CreateWebhookIpRuleRequest,
    IssueApiKeyRequest,
    RotateWebhookSecretRequest,
    ScheduleListResponse,
    TriggerJobRequest,
    UpsertScheduleRequest,
    UpsertWebhookEndpointRequest,
    createWebhookIpRuleRequestSchema,
    issueApiKeyRequestSchema,
    rotateWebhookSecretRequestSchema,
    scheduleListResponseSchema,
    triggerJobRequestSchema,
    upsertScheduleRequestSchema,
    upsertWebhookEndpointRequestSchema,
} from '@croniq/api-schema';

export interface TenantScopedParams {
    tenantId: string;
}

export interface TenantEnvironmentParams extends TenantScopedParams {
    environment: string;
}

interface TenantEnvironmentOptionalParams extends TenantScopedParams {
    environment?: string | null;
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

type QueryRecord = Record<string, string | number | boolean | undefined | null>;

type HttpRequestOptions = {
    params?: Record<string, string>;
    headers: Record<string, string>;
};

type BuildHttpOptionsInput = {
    query?: QueryRecord;
    context?: CallerContext;
};

export const CRONIQ_API_BASE_URL = new InjectionToken<string>('CRONIQ_API_BASE_URL', {
    providedIn: 'root',
    factory: () => 'https://api.croniq.dev',
});

class HttpCroniqApiClient implements CroniqApiClient {
    constructor(private readonly http: HttpClient, private readonly baseUrl: string) { }

    async getSchedules(options?: CroniqRequestOptions): Promise<ScheduleListResponse> {
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        const response$ = this.http
            .get<unknown>(`${this.baseUrl}/schedules`, requestOptions)
            .pipe(map((payload) => scheduleListResponseSchema.parse(payload)));
        return firstValueFrom(response$);
    }

    async upsertSchedule(payload: UpsertScheduleRequest, options?: CroniqRequestOptions): Promise<void> {
        const body = upsertScheduleRequestSchema.parse(payload);
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/schedules`, body, requestOptions));
    }

    async triggerJob(payload: TriggerJobRequest, options?: CroniqRequestOptions): Promise<void> {
        const body = triggerJobRequestSchema.parse(payload);
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/jobs/trigger`, body, requestOptions));
    }

    async issueTenantApiKey(
        params: TenantScopedParams,
        payload: IssueApiKeyRequest,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        const body = issueApiKeyRequestSchema.parse(payload);
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        const response$ = this.http.post<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/api-keys`,
            body,
            requestOptions,
        );
        return firstValueFrom(response$);
    }

    async rotateTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment ?? undefined },
            context: options?.context,
        });
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/api-keys/${params.keyId}/rotate`,
                null,
                requestOptions,
            )
        );
    }

    async deleteTenantApiKey(params: TenantApiKeyParams, options?: CroniqRequestOptions): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment ?? undefined },
            context: options?.context,
        });
        await firstValueFrom(
            this.http.delete<void>(`${this.baseUrl}/tenants/${params.tenantId}/api-keys/${params.keyId}`, requestOptions)
        );
    }

    async getTenantApiClient(params: TenantApiClientParams, options?: CroniqRequestOptions): Promise<unknown> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment ?? undefined },
            context: options?.context,
        });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/api-clients/${params.clientId}`,
            requestOptions,
        );
        return firstValueFrom(response$);
    }

    async listTenantWebhooks(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/webhooks`,
            requestOptions,
        );
        return firstValueFrom(response$);
    }

    async upsertTenantWebhook(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: {
                environment: params.environment,
                allowUnsigned: params.allowUnsigned,
            },
            context: options?.context,
        });
        const body = upsertWebhookEndpointRequestSchema.parse(payload);
        await firstValueFrom(
            this.http.post<void>(`${this.baseUrl}/tenants/${params.tenantId}/webhooks`, body, requestOptions)
        );
    }

    async deleteTenantWebhook(params: TenantWebhookParams, options?: CroniqRequestOptions): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        await firstValueFrom(
            this.http.delete<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}`,
                requestOptions,
            )
        );
    }

    async rotateTenantWebhookSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        const body = rotateWebhookSecretRequestSchema.parse(payload);
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/rotate-secret`,
                body,
                requestOptions,
            )
        );
    }

    async listTenantWebhookIpRules(
        params: TenantWebhookParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/ip-rules`,
            requestOptions,
        );
        return firstValueFrom(response$);
    }

    async createTenantWebhookIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        const body = createWebhookIpRuleRequestSchema.parse(payload);
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/ip-rules`,
                body,
                requestOptions,
            )
        );
    }

    async deleteTenantWebhookIpRule(
        params: TenantWebhookRuleParams,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        await firstValueFrom(
            this.http.delete<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/ip-rules/${params.ruleId}`,
                requestOptions,
            )
        );
    }

    async listTenantWebhookDeadLetters(
        params: TenantEnvironmentParams,
        options?: CroniqRequestOptions,
    ): Promise<unknown> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/webhooks/deadletters`,
            requestOptions,
        );
        return firstValueFrom(response$);
    }

    async replayTenantWebhookDeadLetter(
        params: TenantDeadLetterParams,
        options?: CroniqRequestOptions,
    ): Promise<void> {
        const requestOptions = this.buildHttpOptions({
            query: { environment: params.environment },
            context: options?.context,
        });
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/deadletters/${params.deadLetterId}/replay`,
                null,
                requestOptions,
            )
        );
    }

    async invokeWebhook(params: WebhookInvocationParams, options?: CroniqRequestOptions): Promise<void> {
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/webhooks/${params.hookKey}`, null, requestOptions));
    }

    async fetchExecutionLogs(params: ExecutionLogParams, options?: CroniqRequestOptions): Promise<string> {
        const requestOptions = {
            ...this.buildHttpOptions({ context: options?.context }),
            responseType: 'text' as const,
        };
        const response$ = this.http.get(`${this.baseUrl}/executions/${params.executionId}/logs`, requestOptions);
        return firstValueFrom(response$);
    }

    async checkServiceHealth(options?: CroniqRequestOptions): Promise<void> {
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        await firstValueFrom(this.http.get<void>(`${this.baseUrl}/health`, requestOptions));
    }

    async checkPersistenceHealth(options?: CroniqRequestOptions): Promise<void> {
        const requestOptions = this.buildHttpOptions({ context: options?.context });
        await firstValueFrom(this.http.get<void>(`${this.baseUrl}/health/persistence`, requestOptions));
    }

    private buildHttpOptions(input: BuildHttpOptionsInput = {}): HttpRequestOptions {
        const options: HttpRequestOptions = {
            headers: this.createHeaders(input.context),
        };
        const params = this.createQueryParams(input.query);
        if (params) {
            options.params = params;
        }
        return options;
    }

    private createQueryParams(record?: QueryRecord): Record<string, string> | undefined {
        if (!record) {
            return undefined;
        }
        const entries = Object.entries(record).filter(([, value]) => value !== undefined && value !== null);
        if (!entries.length) {
            return undefined;
        }
        return entries.reduce<Record<string, string>>((acc, [key, value]) => {
            acc[key] = String(value);
            return acc;
        }, {});
    }

    private createHeaders(context?: CallerContext): Record<string, string> {
        const headers: Record<string, string> = {
            'X-Croniq-Client': 'Croniq.Ui',
        };
        if (!context) {
            return headers;
        }
        if (context.source) {
            headers['X-Croniq-Source'] = context.source;
        }
        if (context.actor) {
            headers['X-Croniq-Actor'] = context.actor;
        }
        if (context.tenantId) {
            headers['X-Croniq-Tenant'] = context.tenantId;
        }
        if (context.environment) {
            headers['X-Croniq-Environment'] = context.environment;
        }
        if (context.command) {
            headers['X-Croniq-Command'] = context.command;
        }
        return headers;
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
