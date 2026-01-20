import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { nowIso, nowMs, tryIsoFromUnknown } from '@core/time/clock';
import { CreateWebhookIpRuleRequest, RotateWebhookSecretRequest, UpsertWebhookEndpointRequest, WebhookActivitySummary, WebhookActivityTimelineResponse, type WebhookCapabilitiesResponse } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient, TenantDeadLetterParams, TenantEnvironmentParams, TenantWebhookActivityParams, TenantWebhookActivitySummaryParams, TenantWebhookParams, TenantWebhookRuleParams, TenantWebhookUpsertParams, WebhookInvocationParams } from 'data-access';
import { EMPTY, catchError, forkJoin, fromEvent, map, of, switchMap, takeUntil, tap, timer } from 'rxjs';

export type WebhookEndpointView = {
    hookKey: string;
    jobKey: string;
    environment: string;
    requireSignature: boolean;
    requestsPerMinute?: number;
    metadata?: Record<string, string> | null;
    status: 'active' | 'paused' | 'degraded';
    lastDeliveryAt: string | null;
    ipRuleCount: number | null;
};

export type WebhookCapabilitiesView = {
    allowUnsignedHooks: boolean;
    defaultRequestsPerMinute: number;
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

export type TimelineItemKind = 'delivery' | 'deadLetter';
export type TimelineItemSource = 'ingress' | 'invoke';

export type WebhookTimelineItemView = {
    id: string;
    kind: TimelineItemKind;
    status: 'success' | 'failed' | 'warning';
    label: string;
    occurredAt: string;
    hookKey: string;
    jobKey?: string;
    environment?: string;
    endpointStatus?: WebhookEndpointView['status'];
    reason?: string;
    requestId?: string;
    latencyMs?: number;
    payloadBytes?: number;
    deadLetterId?: string;
    endpointRowKey?: string;
    source?: TimelineItemSource;
};

export type ActivityBucket = {
    bucketStart: string;
    total: number;
    errors: number;
    bucketEnd?: string | null;
    p95LatencyMs?: number | null;
};

export type WebhookActivityQuery = {
    fromUtc?: string | null;
    toUtc?: string | null;
    hookKeys?: ReadonlyArray<string>;
    jobKeys?: ReadonlyArray<string>;
    environment?: string | null;
    limit?: number | null;
    bucketMinutes?: number | null;
};

const ACTIVITY_TIMELINE_LIMIT = 200;
const ACTIVITY_POLL_INTERVAL_MS = 15000;
const EMPTY_ACTIVITY_BUCKETS: ReadonlyArray<ActivityBucket> = [];
const EMPTY_ACTIVITY_TIMELINE: ReadonlyArray<WebhookTimelineItemView> = [];

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
    private readonly capabilitiesSignal = signal<WebhookCapabilitiesView | null>(null);
    private readonly lastErrorSignal = signal<string | null>(null);
    private readonly activityQuerySignal = signal<WebhookActivityQuery | null>(null);
    private readonly activityTimelineSignal = signal<ReadonlyArray<WebhookTimelineItemView>>(EMPTY_ACTIVITY_TIMELINE);
    private readonly activityBucketsSignal = signal<ReadonlyArray<ActivityBucket>>(EMPTY_ACTIVITY_BUCKETS);
    private readonly activityErrorSignal = signal<string | null>(null);
    private readonly activityBackendReadySignal = signal(false);
    private readonly logNextRefreshSignal = signal(false);
    private readonly readPermissionDeniedSignal = signal(false);
    private readonly writePermissionDeniedSignal = signal(false);

    private readonly endpointsResource = tenantRxResource<ReadonlyArray<WebhookEndpointView>, { tenantId: string; environment: string }>({
        command: 'webhooks.list',
        defaultValue: this.endpointsSignal(),
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            this.lastErrorSignal.set(null);
            this.readPermissionDeniedSignal.set(false);

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId) {
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
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:read permissions.',
                    });
                    if (authFailure) {
                        this.lastErrorSignal.set(authFailure.message);
                        this.readPermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                        this.endpointsSignal.set([]);
                        return of(this.endpointsSignal());
                    }
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

    private readonly capabilitiesResource = tenantRxResource<WebhookCapabilitiesView | null, { tenantId: string; environment: string }>({
        command: 'webhooks.capabilities',
        defaultValue: this.capabilitiesSignal(),
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId) {
                return of(this.capabilitiesSignal());
            }

            const request$ = this.api.getTenantWebhookCapabilities({ tenantId, environment }, requestOptions);

            return request$.pipe(
                map((response) => this.normalizeCapabilitiesResponse(response)),
                tap((capabilities) => {
                    this.capabilitiesSignal.set(capabilities);
                    this.readPermissionDeniedSignal.set(false);
                }),
                catchError((error: unknown) => {
                    console.error('Unable to fetch webhook capabilities', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:read permissions.',
                    });
                    if (authFailure) {
                        this.readPermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                        this.lastErrorSignal.set(authFailure.message);
                    }
                    this.capabilitiesSignal.set(null);
                    return of(this.capabilitiesSignal());
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
            if (!tenantId) {
                return of(this.deadLettersSignal().length);
            }

            const request$ = this.api.listTenantWebhookDeadLetters({ tenantId, environment }, requestOptions);

            return request$.pipe(
                map((response) => this.normalizeDeadLettersResponse(response)),
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
            if (!tenantId || !hookKey) {
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

    private readonly activityResource = tenantRxResource<
        { timeline: ReadonlyArray<WebhookTimelineItemView>; buckets: ReadonlyArray<ActivityBucket> },
        { tenantId: string; environment: string; query: WebhookActivityQuery | null }
    >({
        command: 'webhooks.activity',
        defaultValue: {
            timeline: this.activityTimelineSignal(),
            buckets: this.activityBucketsSignal(),
        },
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment, query: this.activityQuerySignal() };
        },
        stream: ({ params, requestOptions, abortSignal }) => {
            this.activityErrorSignal.set(null);
            this.activityBackendReadySignal.set(false);

            const tenantId = params.tenantId.trim();
            const query = params.query;
            if (!tenantId || !query) {
                return of({
                    timeline: this.activityTimelineSignal(),
                    buckets: this.activityBucketsSignal(),
                });
            }

            const normalized = normalizeActivityQuery(query, params.environment);
            const timelineParams: TenantWebhookActivityParams = {
                tenantId,
                environment: normalized.environment,
                fromUtc: normalized.fromUtc,
                toUtc: normalized.toUtc,
                hookKeys: normalized.hookKeys,
                jobKeys: normalized.jobKeys,
                limit: normalized.limit ?? ACTIVITY_TIMELINE_LIMIT,
            };
            const summaryParams: TenantWebhookActivitySummaryParams = {
                tenantId,
                environment: normalized.environment,
                fromUtc: normalized.fromUtc,
                toUtc: normalized.toUtc,
                hookKeys: normalized.hookKeys,
                jobKeys: normalized.jobKeys,
                bucketMinutes: normalized.bucketMinutes,
            };

            const abort$ = fromEvent(abortSignal, 'abort');
            const poll$ = ACTIVITY_POLL_INTERVAL_MS > 0
                ? timer(0, ACTIVITY_POLL_INTERVAL_MS)
                : of(0);

            const fetchTimeline = () =>
                this.api.listTenantWebhookActivity(timelineParams, requestOptions).pipe(
                    map((response) => ({
                        ok: true,
                        value: normalizeActivityTimeline(response),
                    })),
                    catchError((error: unknown) => {
                        console.error('Unable to load webhook activity timeline', error);
                        this.activityErrorSignal.set('Unable to load webhook activity timeline.');
                        return of({
                            ok: false,
                            value: this.activityTimelineSignal(),
                        });
                    }),
                );

            const fetchSummary = () =>
                this.api.getTenantWebhookActivitySummary(summaryParams, requestOptions).pipe(
                    map((response) => ({
                        ok: true,
                        value: normalizeActivityBuckets(response),
                    })),
                    catchError((error: unknown) => {
                        console.error('Unable to load webhook activity summary', error);
                        this.activityErrorSignal.set('Unable to load webhook activity summary.');
                        return of({
                            ok: false,
                            value: this.activityBucketsSignal(),
                        });
                    }),
                );

            return poll$.pipe(
                takeUntil(abort$),
                switchMap(() => forkJoin({ timeline: fetchTimeline(), buckets: fetchSummary() })),
                tap(({ timeline, buckets }) => {
                    this.activityTimelineSignal.set(timeline.value);
                    this.activityBucketsSignal.set(buckets.value);
                    this.activityBackendReadySignal.set(timeline.ok || buckets.ok);
                }),
                map(({ timeline, buckets }) => ({
                    timeline: timeline.value,
                    buckets: buckets.value,
                })),
            );
        },
    });

    readonly endpoints = this.endpointsSignal.asReadonly();
    readonly actionLog = this.actionLogSignal.asReadonly();
    readonly ipRules = this.ipRulesSignal.asReadonly();
    readonly deadLetters = this.deadLettersSignal.asReadonly();
    readonly rotatedSecret = this.rotatedSecretSignal.asReadonly();
    readonly capabilities = this.capabilitiesSignal.asReadonly();
    readonly activityTimeline = this.activityTimelineSignal.asReadonly();
    readonly activityBuckets = this.activityBucketsSignal.asReadonly();
    readonly activityError = this.activityErrorSignal.asReadonly();
    readonly activityBackendReady = this.activityBackendReadySignal.asReadonly();

    readonly loading = computed(() =>
        this.endpointsResource.isLoading()
        || this.deadLetterCountResource.isLoading()
        || this.ipRulesResource.isLoading()
        || this.capabilitiesResource.isLoading(),
    );
    readonly activityLoading = computed(() => this.activityResource.isLoading());

    readonly deadLetterCount = computed(() => this.deadLettersSignal().length);
    readonly lastError = this.lastErrorSignal.asReadonly();
    readonly readPermissionDenied = this.readPermissionDeniedSignal.asReadonly();
    readonly writePermissionDenied = this.writePermissionDeniedSignal.asReadonly();
    readonly activeCount = computed(() => this.endpoints().filter((endpoint) => endpoint.status === 'active').length);

    constructor() {
        queueMicrotask(() => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            this.refreshEndpoints({ tenantId, environment });
        });
    }

    selectHook(hookKey: string): void {
        const normalized = hookKey.trim();
        if (normalized === this.selectedHookKeySignal()) {
            return;
        }
        this.selectedHookKeySignal.set(normalized);
        this.rotatedSecretSignal.set(null);
    }

    setActivityQuery(query: WebhookActivityQuery): void {
        this.activityQuerySignal.set(query);
        this.activityBackendReadySignal.set(false);
    }

    clearRotatedSecret(): void {
        this.rotatedSecretSignal.set(null);
    }

    refreshEndpoints(params: TenantEnvironmentParams): void {
        const tenantId = params.tenantId.trim();
        if (!tenantId) {
            this.lastErrorSignal.set('Required context is missing — unable to load webhooks.');
            return;
        }

        this.readPermissionDeniedSignal.set(false);
        this.logNextRefreshSignal.set(true);
        this.endpointsResource.reload();
        this.deadLetterCountResource.reload();
        this.ipRulesResource.reload();
        this.capabilitiesResource.reload();
        this.activityResource.reload();
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
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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

    setEndpointEnabled(
        params: TenantWebhookUpsertParams,
        endpoint: WebhookEndpointView,
        enabled: boolean,
    ): void {
        const payload: UpsertWebhookEndpointRequest = {
            hookKey: endpoint.hookKey,
            jobKey: endpoint.jobKey,
            enabled,
            requireSignature: endpoint.requireSignature,
            allowUnsigned: !endpoint.requireSignature,
            requestsPerMinute: endpoint.requestsPerMinute ?? null,
            metadata: endpoint.metadata ?? {},
        };

        this.api
            .upsertTenantWebhook(
                params,
                payload,
                this.tenantContext.createRequestOptions('webhooks.toggle', {
                    tenantId: params.tenantId,
                    environment: params.environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.endpointsSignal.update((current) =>
                        current.map((entry) =>
                            entry.hookKey === endpoint.hookKey
                                && entry.environment === (params.environment ?? entry.environment)
                                ? { ...entry, status: enabled ? 'active' : 'paused' }
                                : entry,
                        ),
                    );
                    this.recordAction(`${enabled ? 'Enabled' : 'Disabled'} ${endpoint.hookKey}`, 'success');
                    this.endpointsResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to toggle webhook endpoint', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
                    this.recordAction(
                        `${enabled ? 'Enable' : 'Disable'} failed`,
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
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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
                    this.endpointsResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to create IP rule', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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
                    this.endpointsResource.reload();
                }),
                catchError((error: unknown) => {
                    console.error('Unable to delete IP rule', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing webhooks:write permissions.',
                    });
                    if (authFailure) {
                        this.writePermissionDeniedSignal.set(authFailure.kind === 'forbidden');
                    }
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

    private normalizeDeadLettersResponse(value: unknown): ReadonlyArray<WebhookDeadLetterView> {
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
            const ipRules = record['ipRules'];
            const ipRuleCount = Array.isArray(ipRules) ? ipRules.length : null;
            const metadata = typeof record['metadata'] === 'object' && record['metadata'] !== null
                ? (record['metadata'] as Record<string, string>)
                : null;
            const enabled = typeof record['enabled'] === 'boolean' ? record['enabled'] : true;
            entries.push({
                hookKey: typeof record['hookKey'] === 'string' ? record['hookKey'] : `hook-${index}`,
                jobKey: typeof record['jobKey'] === 'string' ? record['jobKey'] : 'unknown-job',
                environment: typeof record['environment'] === 'string' ? record['environment'] : fallbackEnvironment,
                requireSignature: typeof record['requireSignature'] === 'boolean' ? record['requireSignature'] : true,
                requestsPerMinute: typeof record['requestsPerMinute'] === 'number' ? record['requestsPerMinute'] : undefined,
                status:
                    record['status'] === 'paused' || record['status'] === 'degraded'
                        ? record['status']
                        : enabled
                            ? 'active'
                            : 'paused',
                lastDeliveryAt:
                    tryIsoFromUnknown(record['lastDeliveryAtUtc'] ?? record['lastDeliveryAt']) ?? null,
                ipRuleCount,
                metadata,
            });
        });
        return entries.length ? entries : this.endpointsSignal();
    }

    private normalizeCapabilitiesResponse(value: WebhookCapabilitiesResponse): WebhookCapabilitiesView {
        const allowUnsignedHooks = Boolean(value.allowUnsignedHooks);
        const defaultRequestsPerMinute = typeof value.defaultRequestsPerMinute === 'number'
            && Number.isFinite(value.defaultRequestsPerMinute)
            ? Math.max(1, Math.floor(value.defaultRequestsPerMinute))
            : 60;

        return {
            allowUnsignedHooks,
            defaultRequestsPerMinute,
        };
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

type NormalizedActivityQuery = {
    fromUtc?: string | null;
    toUtc?: string | null;
    hookKeys?: ReadonlyArray<string>;
    jobKeys?: ReadonlyArray<string>;
    environment?: string;
    limit?: number | null;
    bucketMinutes?: number | null;
};

function normalizeActivityQuery(query: WebhookActivityQuery, fallbackEnvironment: string): NormalizedActivityQuery {
    const hookKeys = normalizeKeyList(query.hookKeys);
    const jobKeys = normalizeKeyList(query.jobKeys);
    const fromUtc = tryIsoFromUnknown(query.fromUtc) ?? null;
    const toUtc = tryIsoFromUnknown(query.toUtc) ?? null;
    const environment = resolveEnvironment(query.environment, fallbackEnvironment);
    const limit = normalizePositiveInt(query.limit);
    const bucketMinutes = normalizePositiveInt(query.bucketMinutes);

    return {
        fromUtc,
        toUtc,
        hookKeys,
        jobKeys,
        environment,
        limit,
        bucketMinutes,
    };
}

function normalizeActivityTimeline(entries: WebhookActivityTimelineResponse): ReadonlyArray<WebhookTimelineItemView> {
    if (!Array.isArray(entries)) {
        return EMPTY_ACTIVITY_TIMELINE;
    }

    const mapped = entries.map((entry, index) => {
        const kind = resolveActivityKind(entry.kind);
        const status = resolveActivityStatus(entry.status);
        const source = resolveActivitySource(entry.source);
        const hookKey = resolveNonEmptyString(entry.hookKey) ?? 'unknown-hook';
        const jobKey = resolveNonEmptyString(entry.jobKey);
        const environment = resolveNonEmptyString(entry.environment);
        const occurredAt = tryIsoFromUnknown(entry.occurredAtUtc) ?? nowIso();
        const deadLetterId = resolveDeadLetterId(entry.deadLetterId);
        return {
            id: resolveNonEmptyString(entry.id) ?? `${kind}:${index}`,
            kind,
            status,
            label: resolveActivityLabel(kind, source),
            occurredAt,
            hookKey,
            jobKey,
            environment,
            reason: resolveNonEmptyString(entry.reason),
            requestId: resolveNonEmptyString(entry.requestId),
            latencyMs: resolveOptionalNumber(entry.latencyMs),
            payloadBytes: resolveOptionalNumber(entry.payloadBytes),
            deadLetterId,
            source,
        };
    });

    return mapped.sort((left, right) => right.occurredAt.localeCompare(left.occurredAt));
}

function normalizeActivityBuckets(summary: WebhookActivitySummary): ReadonlyArray<ActivityBucket> {
    if (!summary || !Array.isArray(summary.buckets)) {
        return EMPTY_ACTIVITY_BUCKETS;
    }

    const buckets: ActivityBucket[] = [];
    summary.buckets.forEach((bucket) => {
        const bucketStart = tryIsoFromUnknown(bucket.bucketStartUtc);
        if (!bucketStart) {
            return;
        }
        const bucketEnd = tryIsoFromUnknown(bucket.bucketEndUtc) ?? null;
        const total = resolveOptionalNumber(bucket.totalCount) ?? 0;
        const errors = resolveOptionalNumber(bucket.errorCount) ?? 0;
        const p95LatencyMs = resolveOptionalNumber(bucket.p95LatencyMs);
        buckets.push({
            bucketStart,
            bucketEnd,
            total,
            errors,
            p95LatencyMs,
        });
    });

    if (!buckets.length) {
        return EMPTY_ACTIVITY_BUCKETS;
    }
    return buckets.slice().sort((left, right) => left.bucketStart.localeCompare(right.bucketStart));
}

function resolveNonEmptyString(value: unknown): string | undefined {
    if (typeof value !== 'string') {
        return undefined;
    }
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : undefined;
}

function resolveActivityKind(value: unknown): TimelineItemKind {
    return value === 'deadLetter' ? 'deadLetter' : 'delivery';
}

function resolveActivitySource(value: unknown): TimelineItemSource | undefined {
    if (value === 'invoke') {
        return 'invoke';
    }
    if (value === 'ingress') {
        return 'ingress';
    }
    return undefined;
}

function resolveActivityLabel(kind: TimelineItemKind, source?: TimelineItemSource): string {
    if (kind === 'deadLetter') {
        return 'Dead letter';
    }
    return source === 'invoke' ? 'Manual invoke' : 'Delivery';
}

function resolveActivityStatus(value: unknown): WebhookTimelineItemView['status'] {
    if (value === 'failed') {
        return 'failed';
    }
    if (value === 'warning') {
        return 'warning';
    }
    return 'success';
}

function resolveOptionalNumber(value: unknown): number | undefined {
    return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function resolveDeadLetterId(value: unknown): string | undefined {
    if (typeof value === 'number' && Number.isFinite(value)) {
        return String(value);
    }
    if (typeof value === 'string' && value.trim().length > 0) {
        return value;
    }
    return undefined;
}

function normalizeKeyList(values?: ReadonlyArray<string> | null): ReadonlyArray<string> | undefined {
    if (!values || values.length === 0) {
        return undefined;
    }
    const normalized = values
        .map((value) => value.trim())
        .filter((value) => value.length > 0);
    if (!normalized.length) {
        return undefined;
    }
    return Array.from(new Set(normalized));
}

function normalizePositiveInt(value: number | null | undefined): number | null {
    if (typeof value !== 'number' || !Number.isFinite(value)) {
        return null;
    }
    return Math.max(1, Math.floor(value));
}

function resolveEnvironment(preferred: string | null | undefined, fallback: string): string | undefined {
    if (preferred === null) {
        return undefined;
    }
    const chosen = resolveNonEmptyString(preferred) ?? resolveNonEmptyString(fallback);
    return chosen ?? undefined;
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
