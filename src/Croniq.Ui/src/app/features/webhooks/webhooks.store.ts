import { Injectable, computed, inject, signal } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs, tryIsoFromUnknown } from '@core/time/clock';
import { CreateWebhookIpRuleRequest, RotateWebhookSecretRequest, UpsertWebhookEndpointRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient, TenantDeadLetterParams, TenantEnvironmentParams, TenantWebhookParams, TenantWebhookRuleParams, TenantWebhookUpsertParams, WebhookInvocationParams } from 'data-access';
import { EMPTY, catchError, map, of, tap } from 'rxjs';

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

export type WebhookIpRuleView = {
    ruleId: string;
    cidr: string;
    description?: string;
};

export type WebhookDeadLetterView = {
    id: string;
    hookKey: string;
    jobKey?: string;
    occurredAt: string;
    attempts?: number;
    reason?: string;
};

@Injectable()
export class WebhooksStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly selectedHookKeySignal = signal('');
    private readonly endpointsSignal = signal<ReadonlyArray<WebhookEndpointView>>([]);
    private readonly actionLogSignal = signal<ReadonlyArray<WebhookActionEntry>>([]);
    private readonly ipRulesSignal = signal<ReadonlyArray<WebhookIpRuleView>>([]);
    private readonly deadLettersSignal = signal<ReadonlyArray<WebhookDeadLetterView>>([]);
    private readonly rotatedSecretSignal = signal<string | null>(null);
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

            const request$ = this.api.listTenantWebhooks({ tenantId, environment }, requestOptions);

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
                return of(this.deadLettersSignal().length);
            }

            const request$ = this.api.listTenantWebhookDeadLetters({ tenantId, environment }, requestOptions);

            return request$.pipe(
                map((response) => this.normalizeDeadLettersResponse(response, environment)),
                tap((entries) => this.deadLettersSignal.set(entries)),
                map((entries) => entries.length),
                catchError((error: unknown) => {
                    console.error('Unable to fetch dead letters', error);
                    return of(this.deadLettersSignal().length);
                }),
            );
        },
    });

    private readonly ipRulesResource = tenantRxResource<ReadonlyArray<WebhookIpRuleView>, { tenantId: string; environment: string; hookKey: string }>({
        command: 'webhooks.list-ip-rules',
        defaultValue: this.ipRulesSignal(),
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment, hookKey: this.selectedHookKeySignal() };
        },
        stream: ({ params, requestOptions }) => {
            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            const hookKey = params.hookKey.trim();
            if (!tenantId || !environment || !hookKey) {
                return of(this.ipRulesSignal());
            }

            const request$ = this.api.listTenantWebhookIpRules({ tenantId, environment, hookKey }, requestOptions);

            return request$.pipe(
                map((response) => this.normalizeIpRulesResponse(response)),
                tap((rules) => this.ipRulesSignal.set(rules)),
                catchError((error: unknown) => {
                    console.error('Unable to fetch IP rules', error);
                    return of(this.ipRulesSignal());
                }),
            );
        },
    });

    readonly endpoints = this.endpointsSignal.asReadonly();
    readonly actionLog = this.actionLogSignal.asReadonly();
    readonly ipRules = this.ipRulesSignal.asReadonly();
    readonly deadLetters = this.deadLettersSignal.asReadonly();
    readonly rotatedSecret = this.rotatedSecretSignal.asReadonly();

    readonly loading = computed(() =>
        this.endpointsResource.isLoading()
        || this.deadLetterCountResource.isLoading()
        || this.ipRulesResource.isLoading(),
    );

    readonly deadLetterCount = computed(() => this.deadLettersSignal().length);
    readonly lastError = this.lastErrorSignal.asReadonly();
    readonly activeCount = computed(() => this.endpoints().filter((endpoint) => endpoint.status === 'active').length);

    selectHook(hookKey: string): void {
        const normalized = hookKey.trim();
        if (normalized === this.selectedHookKeySignal()) {
            return;
        }
        this.selectedHookKeySignal.set(normalized);
        this.rotatedSecretSignal.set(null);
    }

    refreshEndpoints(params: TenantEnvironmentParams): void {
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
        this.ipRulesResource.reload();
    }

    upsertEndpoint(
        params: TenantWebhookUpsertParams,
        payload: UpsertWebhookEndpointRequest,
    ): void {
        this.api
            .upsertTenantWebhook(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.upsert', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.recordAction(`Upserted ${payload.hookKey}`, 'success');
                    this.endpointsResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to upsert webhook', error);
                    this.recordAction(
                        'Upsert failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    deleteEndpoint(params: TenantWebhookParams): void {
        this.api
            .deleteTenantWebhook(
                params,
                this.tenantContext.createRequestOptions('webhooks.delete', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.recordAction(`Deleted ${params.hookKey}`, 'success');
                    this.endpointsResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to delete webhook', error);
                    this.recordAction(
                        'Delete failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    rotateSecret(
        params: TenantWebhookParams,
        payload: RotateWebhookSecretRequest,
    ): void {
        this.api
            .rotateTenantWebhookSecret(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.rotate-secret', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap((response) => {
                    const rotatedSecret = extractRotatedSecret(response);
                    this.rotatedSecretSignal.set(rotatedSecret);
                    this.recordAction(`Rotated secret for ${params.hookKey}`, 'success');
                    this.endpointsResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to rotate webhook secret', error);
                    this.recordAction(
                        'Secret rotation failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    createIpRule(
        params: TenantWebhookParams,
        payload: CreateWebhookIpRuleRequest,
    ): void {
        this.api
            .createTenantWebhookIpRule(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.create-ip-rule', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.recordAction(`Created IP rule for ${params.hookKey}`, 'success');
                    this.ipRulesResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to create IP rule', error);
                    this.recordAction(
                        'IP rule creation failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    deleteIpRule(params: TenantWebhookRuleParams): void {
        this.api
            .deleteTenantWebhookIpRule(
                params,
                this.tenantContext.createRequestOptions('webhooks.delete-ip-rule', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.recordAction(`Removed IP rule ${params.ruleId}`, 'success');
                    this.ipRulesResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to delete IP rule', error);
                    this.recordAction(
                        'IP rule deletion failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    replayDeadLetter(params: TenantDeadLetterParams): void {
        this.api
            .replayTenantWebhookDeadLetter(
                params,
                this.tenantContext.createRequestOptions('webhooks.replay-dead-letter', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.recordAction(`Replayed dead letter ${params.deadLetterId}`, 'success');
                    this.deadLetterCountResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to replay dead letter', error);
                    this.recordAction(
                        'Dead letter replay failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    invokeWebhook(params: WebhookInvocationParams): void {
        this.api
            .invokeWebhook(params, this.tenantContext.createRequestOptions(`webhooks.invoke:${params.hookKey}`))
            .pipe(
                tap(() => {
                    this.recordAction(`Invoked ${params.hookKey}`, 'success');
                }),
                catchError((error: unknown) => {
                    console.error('Unable to invoke webhook', error);
                    this.recordAction(
                        'Invocation failed',
                        'error',
                        error instanceof Error ? error.message : 'Unknown error',
                    );
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    private normalizeDeadLettersResponse(value: unknown, fallbackEnvironment: string): ReadonlyArray<WebhookDeadLetterView> {
        if (!Array.isArray(value)) {
            return this.deadLettersSignal();
        }

        const entries: WebhookDeadLetterView[] = [];
        value.forEach((item, index) => {
            if (typeof item !== 'object' || item === null) {
                return;
            }
            const record = item as Record<string, unknown>;
            const id = record['id'] ?? record['deadLetterId'] ?? index;
            entries.push({
                id: String(id),
                hookKey: typeof record['hookKey'] === 'string' ? record['hookKey'] : 'unknown-hook',
                jobKey: typeof record['jobKey'] === 'string' ? record['jobKey'] : undefined,
                occurredAt: tryIsoFromUnknown(record['occurredAt'] ?? record['occurredAtUtc'] ?? record['createdAt'] ?? record['createdAtUtc']) ?? nowIso(),
                attempts: typeof record['attempts'] === 'number' ? record['attempts'] : undefined,
                reason: typeof record['reason'] === 'string' ? record['reason'] : typeof record['error'] === 'string' ? record['error'] : undefined,
            });
        });

        return entries.length ? entries : this.deadLettersSignal();
    }

    private normalizeIpRulesResponse(value: unknown): ReadonlyArray<WebhookIpRuleView> {
        if (!Array.isArray(value)) {
            return this.ipRulesSignal();
        }
        const entries: WebhookIpRuleView[] = [];
        value.forEach((item, index) => {
            if (typeof item !== 'object' || item === null) {
                return;
            }
            const record = item as Record<string, unknown>;
            const id = record['ruleId'] ?? record['id'] ?? index;
            entries.push({
                ruleId: String(id),
                cidr: typeof record['cidr'] === 'string' ? record['cidr'] : 'unknown-cidr',
                description: typeof record['description'] === 'string' ? record['description'] : undefined,
            });
        });
        return entries.length ? entries : this.ipRulesSignal();
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

function extractRotatedSecret(payload: unknown): string | null {
    if (typeof payload === 'string') {
        return payload.trim() ? payload : null;
    }
    if (typeof payload !== 'object' || payload === null) {
        return null;
    }
    const record = payload as Record<string, unknown>;
    const candidates: ReadonlyArray<unknown> = [
        record['secret'],
        record['plaintextSecret'],
        record['plainTextSecret'],
        record['value'],
    ];
    const hit = candidates.find((value) => typeof value === 'string' && value.trim().length > 0);
    return typeof hit === 'string' ? hit : null;
}

function createEntryId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${nowMs()}-${Math.round(Math.random() * 1000)}`;
}
