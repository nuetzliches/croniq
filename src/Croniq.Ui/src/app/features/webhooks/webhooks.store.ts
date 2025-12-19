import { Injectable, computed, inject, signal } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs, tryIsoFromUnknown } from '@core/time/clock';
import { CreateWebhookIpRuleRequest, RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient, TenantDeadLetterParams, TenantEnvironmentParams, TenantWebhookParams, TenantWebhookRuleParams, TenantWebhookUpsertParams, WebhookInvocationParams } from 'data-access';
import { catchError, from, map, of, tap } from 'rxjs';

export type WebhookEndpointView = {
    hookKey: string;
    jobKey: string;
    environment: string;
    requireSignature: boolean;
    requestsPerMinute?: number;
    status: 'active' | 'paused' | 'degraded';
    lastDeliveryAt: string;
};

export type WebhookActionEntry = {
    id: string;
    summary: string;
    status: 'success' | 'error';
    detail?: string;
    recordedAt: string;
};

@Injectable()
export class WebhooksStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly endpointsSignal = signal<ReadonlyArray<WebhookEndpointView>>(seedEndpoints());
    private readonly actionLogSignal = signal<ReadonlyArray<WebhookActionEntry>>(seedActionLog());
    private readonly deadLetterCountSignal = signal(0);
    private readonly lastErrorSignal = signal<string | null>(null);
    private readonly logNextRefreshSignal = signal(false);

    private readonly endpointsResource = tenantRxResource<ReadonlyArray<WebhookEndpointView>, { tenantId: string; environment: string }>({
        command: 'webhooks.list',
        defaultValue: this.endpointsSignal(),
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            this.lastErrorSignal.set(null);

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId || !environment) {
                return of(this.endpointsSignal());
            }

            const request$ = this.api.listTenantWebhooks$
                ? this.api.listTenantWebhooks$({ tenantId, environment }, requestOptions)
                : from(this.api.listTenantWebhooks({ tenantId, environment }, requestOptions));

            return request$.pipe(
                map((response) => this.normalizeEndpointResponse(response, environment)),
                tap((normalized) => {
                    this.endpointsSignal.set(normalized);
                    if (this.logNextRefreshSignal()) {
                        this.logNextRefreshSignal.set(false);
                        this.recordAction('Refreshed webhook endpoints', 'success');
                    }
                }),
                catchError((error: unknown) => {
                    console.error('Unable to refresh webhooks', error);
                    this.lastErrorSignal.set('Failed to load endpoints from API.');
                    if (this.logNextRefreshSignal()) {
                        this.logNextRefreshSignal.set(false);
                        this.recordAction(
                            'Refresh failed',
                            'error',
                            error instanceof Error ? error.message : 'Unknown error',
                        );
                    }
                    return of(this.endpointsSignal());
                }),
            );
        },
    });

    private readonly deadLetterCountResource = tenantRxResource<number, { tenantId: string; environment: string }>({
        command: 'webhooks.list-dead-letters',
        defaultValue: 0,
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId || !environment) {
                return of(this.deadLetterCountSignal());
            }

            const request$ = this.api.listTenantWebhookDeadLetters$
                ? this.api.listTenantWebhookDeadLetters$({ tenantId, environment }, requestOptions)
                : from(this.api.listTenantWebhookDeadLetters({ tenantId, environment }, requestOptions));

            return request$.pipe(
                map((response) => (Array.isArray(response) ? response.length : this.deadLetterCountSignal())),
                tap((total) => this.deadLetterCountSignal.set(total)),
                catchError((error: unknown) => {
                    console.error('Unable to fetch dead letters', error);
                    return of(this.deadLetterCountSignal());
                }),
            );
        },
    });

    readonly endpoints = this.endpointsSignal.asReadonly();
    readonly actionLog = this.actionLogSignal.asReadonly();
    readonly loading = computed(() => this.endpointsResource.isLoading() || this.deadLetterCountResource.isLoading());
    readonly deadLetterCount = this.deadLetterCountSignal.asReadonly();
    readonly lastError = this.lastErrorSignal.asReadonly();
    readonly activeCount = computed(() => this.endpoints().filter((endpoint) => endpoint.status === 'active').length);

    async refreshEndpoints(params: TenantEnvironmentParams): Promise<void> {
        const tenantId = params.tenantId.trim();
        const environment = params.environment.trim();
        if (!tenantId) {
            this.lastErrorSignal.set('TenantId is not set — select a tenant to load webhooks.');
            return;
        }
        if (!environment) {
            this.lastErrorSignal.set('Environment is not set — select an environment to load webhooks.');
            return;
        }

        this.logNextRefreshSignal.set(true);
        this.endpointsResource.reload();
        this.deadLetterCountResource.reload();
    }

    async upsertEndpoint(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
    ): Promise<void> {
        try {
            await this.api.upsertTenantWebhook(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.upsert', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            );
            this.recordAction(`Upserted ${payload.hookKey}`, 'success');
        } catch (error) {
            console.error('Unable to upsert webhook', error);
            this.recordAction('Upsert failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    async deleteEndpoint(params: TenantWebhookParams): Promise<void> {
        try {
            await this.api.deleteTenantWebhook(
                params,
                this.tenantContext.createRequestOptions('webhooks.delete', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            );
            this.recordAction(`Deleted ${params.hookKey}`, 'success');
        } catch (error) {
            console.error('Unable to delete webhook', error);
            this.recordAction('Delete failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    async rotateSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
    ): Promise<void> {
        try {
            await this.api.rotateTenantWebhookSecret(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.rotate-secret', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            );
            this.recordAction(`Rotated secret for ${params.hookKey}`, 'success');
        } catch (error) {
            console.error('Unable to rotate webhook secret', error);
            this.recordAction('Secret rotation failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    async createIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
    ): Promise<void> {
        try {
            await this.api.createTenantWebhookIpRule(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.create-ip-rule', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            );
            this.recordAction(`Created IP rule for ${params.hookKey}`, 'success');
        } catch (error) {
            console.error('Unable to create IP rule', error);
            this.recordAction('IP rule creation failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    async deleteIpRule(params: TenantWebhookRuleParams): Promise<void> {
        try {
            await this.api.deleteTenantWebhookIpRule(
                params,
                this.tenantContext.createRequestOptions('webhooks.delete-ip-rule', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            );
            this.recordAction(`Removed IP rule ${params.ruleId}`, 'success');
        } catch (error) {
            console.error('Unable to delete IP rule', error);
            this.recordAction('IP rule deletion failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    async replayDeadLetter(params: TenantDeadLetterParams): Promise<void> {
        try {
            await this.api.replayTenantWebhookDeadLetter(
                params,
                this.tenantContext.createRequestOptions('webhooks.replay-dead-letter', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            );
            this.recordAction(`Replayed dead letter ${params.deadLetterId}`, 'success');
        } catch (error) {
            console.error('Unable to replay dead letter', error);
            this.recordAction('Dead letter replay failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    async invokeWebhook(params: WebhookInvocationParams): Promise<void> {
        try {
            await this.api.invokeWebhook(
                params,
                this.tenantContext.createRequestOptions(`webhooks.invoke:${params.hookKey}`),
            );
            this.recordAction(`Invoked ${params.hookKey}`, 'success');
        } catch (error) {
            console.error('Unable to invoke webhook', error);
            this.recordAction('Invocation failed', 'error', error instanceof Error ? error.message : 'Unknown error');
        }
    }

    private normalizeEndpointResponse(value: unknown, fallbackEnvironment: string): ReadonlyArray<WebhookEndpointView> {
        if (!Array.isArray(value)) {
            return this.endpointsSignal();
        }
        const entries: WebhookEndpointView[] = [];
        value.forEach((item, index) => {
            if (typeof item !== 'object' || item === null) {
                return;
            }
            const record = item as Record<string, unknown>;
            entries.push({
                hookKey: typeof record['hookKey'] === 'string' ? record['hookKey'] : `hook-${index}`,
                jobKey: typeof record['jobKey'] === 'string' ? record['jobKey'] : 'unknown-job',
                environment: typeof record['environment'] === 'string' ? record['environment'] : fallbackEnvironment,
                requireSignature: typeof record['requireSignature'] === 'boolean' ? record['requireSignature'] : true,
                requestsPerMinute: typeof record['requestsPerMinute'] === 'number' ? record['requestsPerMinute'] : undefined,
                status:
                    record['status'] === 'paused' || record['status'] === 'degraded'
                        ? record['status']
                        : 'active',
                lastDeliveryAt:
                    tryIsoFromUnknown(record['lastDeliveryAt']) ?? nowIso(),
            });
        });
        return entries.length ? entries : this.endpointsSignal();
    }

    private recordAction(summary: string, status: 'success' | 'error', detail?: string): void {
        const entry: WebhookActionEntry = {
            id: createEntryId(),
            summary,
            status,
            detail,
            recordedAt: nowIso(),
        };
        this.actionLogSignal.set([entry, ...this.actionLogSignal()].slice(0, 20));
    }
}

function seedEndpoints(): ReadonlyArray<WebhookEndpointView> {
    const now = nowMs();
    return [
        {
            hookKey: 'billing-updates',
            jobKey: 'jobs.billing-webhook',
            environment: 'production',
            requireSignature: true,
            requestsPerMinute: 120,
            status: 'active',
            lastDeliveryAt: isoFromEpochMs(now - 1000 * 60 * 5),
        },
        {
            hookKey: 'ops-dead-letter',
            jobKey: 'jobs.ops-dead-letter',
            environment: 'staging',
            requireSignature: true,
            requestsPerMinute: 40,
            status: 'degraded',
            lastDeliveryAt: isoFromEpochMs(now - 1000 * 60 * 45),
        },
    ];
}

function seedActionLog(): ReadonlyArray<WebhookActionEntry> {
    const now = nowMs();
    return [
        {
            id: createEntryId(),
            summary: 'Secret rotated for billing-updates',
            status: 'success',
            recordedAt: isoFromEpochMs(now - 1000 * 60 * 60),
        },
    ];
}

function createEntryId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${nowMs()}-${Math.round(Math.random() * 1000)}`;
}
