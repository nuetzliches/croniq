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

export interface CroniqApiClient {
    getSchedules(): Promise<ScheduleListResponse>;
    upsertSchedule(payload: UpsertScheduleRequest): Promise<void>;
    triggerJob(payload: TriggerJobRequest): Promise<void>;
    issueTenantApiKey(params: TenantScopedParams, payload: IssueApiKeyRequest): Promise<unknown>;
    rotateTenantApiKey(params: TenantApiKeyParams): Promise<void>;
    deleteTenantApiKey(params: TenantApiKeyParams): Promise<void>;
    getTenantApiClient(params: TenantApiClientParams): Promise<unknown>;
    listTenantWebhooks(params: TenantEnvironmentParams): Promise<unknown>;
    upsertTenantWebhook(params: TenantWebhookUpsertParams, payload: UpsertWebhookEndpointRequest): Promise<void>;
    deleteTenantWebhook(params: TenantWebhookParams): Promise<void>;
    rotateTenantWebhookSecret(params: TenantWebhookParams, payload: RotateWebhookSecretRequest): Promise<void>;
    listTenantWebhookIpRules(params: TenantWebhookParams): Promise<unknown>;
    createTenantWebhookIpRule(params: TenantWebhookParams, payload: CreateWebhookIpRuleRequest): Promise<void>;
    deleteTenantWebhookIpRule(params: TenantWebhookRuleParams): Promise<void>;
    listTenantWebhookDeadLetters(params: TenantEnvironmentParams): Promise<unknown>;
    replayTenantWebhookDeadLetter(params: TenantDeadLetterParams): Promise<void>;
    invokeWebhook(params: WebhookInvocationParams): Promise<void>;
    fetchExecutionLogs(params: ExecutionLogParams): Promise<string>;
    checkServiceHealth(): Promise<void>;
    checkPersistenceHealth(): Promise<void>;
}

export const CRONIQ_API_BASE_URL = new InjectionToken<string>('CRONIQ_API_BASE_URL', {
    providedIn: 'root',
    factory: () => 'https://api.croniq.dev',
});

class HttpCroniqApiClient implements CroniqApiClient {
    constructor(private readonly http: HttpClient, private readonly baseUrl: string) { }

    async getSchedules(): Promise<ScheduleListResponse> {
        const response$ = this.http
            .get<unknown>(`${this.baseUrl}/schedules`)
            .pipe(map((payload) => scheduleListResponseSchema.parse(payload)));
        return firstValueFrom(response$);
    }

    async upsertSchedule(payload: UpsertScheduleRequest): Promise<void> {
        const body = upsertScheduleRequestSchema.parse(payload);
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/schedules`, body));
    }

    async triggerJob(payload: TriggerJobRequest): Promise<void> {
        const body = triggerJobRequestSchema.parse(payload);
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/jobs/trigger`, body));
    }

    async issueTenantApiKey(params: TenantScopedParams, payload: IssueApiKeyRequest): Promise<unknown> {
        const body = issueApiKeyRequestSchema.parse(payload);
        const response$ = this.http.post<unknown>(`${this.baseUrl}/tenants/${params.tenantId}/api-keys`, body);
        return firstValueFrom(response$);
    }

    async rotateTenantApiKey(params: TenantApiKeyParams): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment ?? undefined });
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/api-keys/${params.keyId}/rotate`,
                null,
                options
            )
        );
    }

    async deleteTenantApiKey(params: TenantApiKeyParams): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment ?? undefined });
        await firstValueFrom(
            this.http.delete<void>(`${this.baseUrl}/tenants/${params.tenantId}/api-keys/${params.keyId}`, options)
        );
    }

    async getTenantApiClient(params: TenantApiClientParams): Promise<unknown> {
        const options = this.buildQueryOptions({ environment: params.environment ?? undefined });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/api-clients/${params.clientId}`,
            options
        );
        return firstValueFrom(response$);
    }

    async listTenantWebhooks(params: TenantEnvironmentParams): Promise<unknown> {
        const options = this.buildQueryOptions({ environment: params.environment });
        const response$ = this.http.get<unknown>(`${this.baseUrl}/tenants/${params.tenantId}/webhooks`, options);
        return firstValueFrom(response$);
    }

    async upsertTenantWebhook(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest
    ): Promise<void> {
        const options = this.buildQueryOptions({
            environment: params.environment,
            allowUnsigned: params.allowUnsigned,
        });
        const body = upsertWebhookEndpointRequestSchema.parse(payload);
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/tenants/${params.tenantId}/webhooks`, body, options));
    }

    async deleteTenantWebhook(params: TenantWebhookParams): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment });
        await firstValueFrom(
            this.http.delete<void>(`${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}`, options)
        );
    }

    async rotateTenantWebhookSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest
    ): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment });
        const body = rotateWebhookSecretRequestSchema.parse(payload);
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/rotate-secret`,
                body,
                options
            )
        );
    }

    async listTenantWebhookIpRules(params: TenantWebhookParams): Promise<unknown> {
        const options = this.buildQueryOptions({ environment: params.environment });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/ip-rules`,
            options
        );
        return firstValueFrom(response$);
    }

    async createTenantWebhookIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest
    ): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment });
        const body = createWebhookIpRuleRequestSchema.parse(payload);
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/ip-rules`,
                body,
                options
            )
        );
    }

    async deleteTenantWebhookIpRule(params: TenantWebhookRuleParams): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment });
        await firstValueFrom(
            this.http.delete<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/${params.hookKey}/ip-rules/${params.ruleId}`,
                options
            )
        );
    }

    async listTenantWebhookDeadLetters(params: TenantEnvironmentParams): Promise<unknown> {
        const options = this.buildQueryOptions({ environment: params.environment });
        const response$ = this.http.get<unknown>(
            `${this.baseUrl}/tenants/${params.tenantId}/webhooks/deadletters`,
            options
        );
        return firstValueFrom(response$);
    }

    async replayTenantWebhookDeadLetter(params: TenantDeadLetterParams): Promise<void> {
        const options = this.buildQueryOptions({ environment: params.environment });
        await firstValueFrom(
            this.http.post<void>(
                `${this.baseUrl}/tenants/${params.tenantId}/webhooks/deadletters/${params.deadLetterId}/replay`,
                null,
                options
            )
        );
    }

    async invokeWebhook(params: WebhookInvocationParams): Promise<void> {
        await firstValueFrom(this.http.post<void>(`${this.baseUrl}/webhooks/${params.hookKey}`, null));
    }

    async fetchExecutionLogs(params: ExecutionLogParams): Promise<string> {
        const response$ = this.http.get(`${this.baseUrl}/executions/${params.executionId}/logs`, {
            responseType: 'text',
        });
        return firstValueFrom(response$);
    }

    async checkServiceHealth(): Promise<void> {
        await firstValueFrom(this.http.get<void>(`${this.baseUrl}/health`));
    }

    async checkPersistenceHealth(): Promise<void> {
        await firstValueFrom(this.http.get<void>(`${this.baseUrl}/health/persistence`));
    }

    private buildQueryOptions(
        record: Record<string, string | number | boolean | undefined | null>
    ): { params: Record<string, string> } | undefined {
        const entries = Object.entries(record).filter(([, value]) => value !== undefined && value !== null);
        if (!entries.length) {
            return undefined;
        }
        const params = entries.reduce<Record<string, string>>((acc, [key, value]) => {
            acc[key] = String(value);
            return acc;
        }, {});
        return { params };
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
