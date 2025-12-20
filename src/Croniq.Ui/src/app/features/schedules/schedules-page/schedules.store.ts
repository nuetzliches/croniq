import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs } from '@core/time/clock';
import { ScheduleListResponse, ScheduleSummary, UpsertScheduleRequest, scheduleListResponseSchema } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { EMPTY, catchError, finalize, map, of, tap } from 'rxjs';

export type ScheduleDetail = {
    triggerId: string;
    jobKey?: string;
    cronExpression?: string;
    enabled?: boolean;
    startAtUtc?: string;
    endAtUtc?: string;
    description?: string;
    name?: string;
    cron?: string;
    timezone?: string;
    state?: string;
};

export type ScheduleDeadLetterView = {
    id: number;
    triggerId?: string;
    jobKey?: string;
    occurredAtUtc?: string;
    detail?: string;
};

@Injectable()
export class SchedulesStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly schedulesSignal = signal<ReadonlyArray<ScheduleSummary>>([]);
    private readonly lastUpdatedSignal = signal<string>(nowIso());
    readonly error = signal<string | null>(null);

    private readonly schedulesResource = tenantRxResource<ScheduleListResponse, { tenantId: string }>({
        command: 'schedules.refresh',
        defaultValue: createFallbackResponse(),
        params: () => ({
            tenantId: this.tenantContext.snapshot().tenantId,
        }),
        stream: ({ params, requestOptions }) => {
            this.error.set(null);

            const tenantId = params.tenantId.trim();
            if (!tenantId) {
                const fallback = createFallbackResponse();
                this.hydrate(fallback);
                this.error.set('TenantId is not set — select a tenant to load schedules.');
                return of(fallback);
            }

            const request$ = this.api.getSchedules({ tenantId }, requestOptions);

            return request$.pipe(
                tap((response) => {
                    this.hydrate(response);
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load schedules', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden:
                            'Forbidden (403) — your token is missing schedules permissions for this tenant.',
                    });
                    if (authFailure) {
                        this.error.set(authFailure.message);
                        const empty = createEmptyResponse();
                        this.hydrate(empty);
                        return of(empty);
                    }

                    this.error.set('Unable to load schedules from API — showing fallback data.');
                    const fallback = createFallbackResponse();
                    this.hydrate(fallback);
                    return of(fallback);
                }),
            );
        },
    });

    readonly loading = computed(() => this.schedulesResource.isLoading());

    private readonly scheduleDetailSignal = signal<ScheduleDetail | null>(null);
    private readonly scheduleDetailTriggerIdSignal = signal<string | null>(null);
    private readonly scheduleDetailErrorSignal = signal<string | null>(null);
    private readonly scheduleDetailResource = tenantRxResource<ScheduleDetail | null, { tenantId: string; environment: string; triggerId: string | null }>({
        command: 'schedules.get',
        defaultValue: null,
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return {
                tenantId,
                environment,
                triggerId: this.scheduleDetailTriggerIdSignal(),
            };
        },
        stream: ({ params, requestOptions }) => {
            this.scheduleDetailErrorSignal.set(null);

            const trimmedId = params.triggerId?.trim() ?? '';
            if (!trimmedId) {
                // Nothing selected -> nothing to load.
                this.scheduleDetailSignal.set(null);
                return of(null);
            }

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId) {
                this.scheduleDetailErrorSignal.set('TenantId is not set — select a tenant to load schedule detail.');
                this.scheduleDetailSignal.set(null);
                return of(null);
            }
            if (!environment) {
                this.scheduleDetailErrorSignal.set(
                    'Environment is not set — select an environment to load schedule detail.',
                );
                this.scheduleDetailSignal.set(null);
                return of(null);
            }

            const request$ = this.api.getSchedule({ tenantId, environment, triggerId: trimmedId }, requestOptions);

            return request$.pipe(
                map((response) => normalizeScheduleDetail(response, trimmedId)),
                tap((normalized) => {
                    this.scheduleDetailSignal.set(normalized);
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load schedule detail', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing schedules permissions for this tenant.',
                    });
                    if (authFailure) {
                        this.scheduleDetailErrorSignal.set(authFailure.message);
                        this.scheduleDetailSignal.set(null);
                        return of(null);
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.scheduleDetailErrorSignal.set('Schedule not found (404) — verify the trigger id.');
                        this.scheduleDetailSignal.set(null);
                        return of(null);
                    }
                    this.scheduleDetailErrorSignal.set('Unable to load schedule detail from API.');
                    this.scheduleDetailSignal.set(null);
                    return of(null);
                }),
            );
        },
    });

    private readonly deleteScheduleLoadingSignal = signal(false);
    private readonly deleteScheduleErrorSignal = signal<string | null>(null);

    private readonly upsertScheduleLoadingSignal = signal(false);
    private readonly upsertScheduleErrorSignal = signal<string | null>(null);

    private readonly scheduleDeadLettersSignal = signal<ReadonlyArray<ScheduleDeadLetterView>>([]);
    private readonly scheduleDeadLettersErrorSignal = signal<string | null>(null);
    private readonly scheduleDeadLettersResource = tenantRxResource<ReadonlyArray<ScheduleDeadLetterView>, { tenantId: string; environment: string }>(
        {
            command: 'schedules.list-dead-letters',
            defaultValue: [],
            params: () => {
                const { tenantId, environment } = this.tenantContext.snapshot();
                return { tenantId, environment };
            },
            stream: ({ params, requestOptions }) => {
                this.scheduleDeadLettersErrorSignal.set(null);

                const tenantId = params.tenantId.trim();
                const environment = params.environment.trim();
                if (!tenantId || !environment) {
                    this.scheduleDeadLettersSignal.set([]);
                    return of([]);
                }

                return this.api.listTenantScheduleDeadLetters({ tenantId, environment }, requestOptions).pipe(
                    map((response) => normalizeScheduleDeadLettersResponse(response)),
                    tap((entries) => this.scheduleDeadLettersSignal.set(entries)),
                    catchError((error: unknown) => {
                        console.error('Failed to load schedule dead letters', error);
                        this.scheduleDeadLettersErrorSignal.set('Unable to load schedule dead letters from API.');
                        const current = this.scheduleDeadLettersSignal();
                        return of(current);
                    }),
                );
            },
        },
    );

    readonly schedules = this.schedulesSignal.asReadonly();
    readonly lastUpdated = this.lastUpdatedSignal.asReadonly();

    readonly scheduleDetail = this.scheduleDetailSignal.asReadonly();
    readonly scheduleDetailLoading = computed(() => this.scheduleDetailResource.isLoading());
    readonly scheduleDetailError = this.scheduleDetailErrorSignal.asReadonly();
    readonly deleteScheduleLoading = this.deleteScheduleLoadingSignal.asReadonly();
    readonly deleteScheduleError = this.deleteScheduleErrorSignal.asReadonly();

    readonly upsertScheduleLoading = this.upsertScheduleLoadingSignal.asReadonly();
    readonly upsertScheduleError = this.upsertScheduleErrorSignal.asReadonly();

    readonly scheduleDeadLetters = this.scheduleDeadLettersSignal.asReadonly();
    readonly scheduleDeadLettersLoading = computed(() => this.scheduleDeadLettersResource.isLoading());
    readonly scheduleDeadLettersError = this.scheduleDeadLettersErrorSignal.asReadonly();
    readonly scheduleDeadLetterCount = computed(() => this.scheduleDeadLettersSignal().length);

    constructor() {
        this.hydrate(createFallbackResponse());
    }

    refresh(): void {
        this.schedulesResource.reload();
    }

    refreshScheduleDetail(triggerId: string): void {
        const trimmedId = triggerId.trim();
        if (!trimmedId) {
            this.scheduleDetailErrorSignal.set('Trigger id is required to load schedule detail.');
            this.scheduleDetailTriggerIdSignal.set(null);
            this.scheduleDetailSignal.set(null);
            return;
        }

        this.scheduleDetailTriggerIdSignal.set(trimmedId);
        this.scheduleDetailResource.reload();
    }

    deleteSchedule(triggerId: string): void {
        const trimmedId = triggerId.trim();
        if (!trimmedId) {
            this.deleteScheduleErrorSignal.set('Trigger id is required before deleting.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.deleteScheduleErrorSignal.set('TenantId is not set — select a tenant to delete schedules.');
            return;
        }
        if (!environment.trim()) {
            this.deleteScheduleErrorSignal.set('Environment is not set — select an environment to delete schedules.');
            return;
        }

        this.deleteScheduleLoadingSignal.set(true);
        this.deleteScheduleErrorSignal.set(null);

        this.api
            .deleteSchedule(
                { tenantId, environment, triggerId: trimmedId },
                this.tenantContext.createRequestOptions('schedules.delete', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    const current = this.scheduleDetailSignal();
                    if (current?.triggerId === trimmedId) {
                        this.scheduleDetailSignal.set(null);
                    }
                    this.schedulesSignal.set(this.schedulesSignal().filter((schedule) => schedule.id !== trimmedId));
                    this.refresh();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to delete schedule', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing schedules permissions for this tenant.',
                    });
                    if (authFailure) {
                        this.deleteScheduleErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.deleteScheduleErrorSignal.set(
                            'Schedule not found (404) — it may have already been deleted.',
                        );
                        return EMPTY;
                    }
                    this.deleteScheduleErrorSignal.set('Unable to delete schedule via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.deleteScheduleLoadingSignal.set(false);
                }),
            )
            .subscribe();
    }

    upsertSchedule(payload: UpsertScheduleRequest): void {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.upsertScheduleErrorSignal.set('TenantId is not set — select a tenant to upsert schedules.');
            return;
        }
        if (!environment.trim()) {
            this.upsertScheduleErrorSignal.set('Environment is not set — select an environment to upsert schedules.');
            return;
        }

        // Safety net: when updating a selected schedule, always keep its triggerId unless the caller provided one.
        // Without triggerId, the backend derives `{jobKey}:{cronExpression}` which would create a new trigger if the cron changes.
        const selected = this.scheduleDetailSignal();
        const normalizedTriggerId = typeof payload.triggerId === 'string' ? payload.triggerId.trim() : '';
        const safePayload: UpsertScheduleRequest = {
            ...payload,
            triggerId: normalizedTriggerId ? normalizedTriggerId : selected?.triggerId,
        };

        this.upsertScheduleLoadingSignal.set(true);
        this.upsertScheduleErrorSignal.set(null);

        this.api
            .upsertSchedule(
                { tenantId, environment },
                safePayload,
                this.tenantContext.createRequestOptions('schedules.upsert', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.refresh();
                    const triggerId = safePayload.triggerId;
                    if (triggerId) {
                        this.refreshScheduleDetail(triggerId);
                    }
                }),
                catchError((error: unknown) => {
                    console.error('Failed to upsert schedule', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing schedules permissions for this tenant.',
                    });
                    if (authFailure) {
                        this.upsertScheduleErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    this.upsertScheduleErrorSignal.set('Unable to upsert schedule via API.');
                    return EMPTY;
                }),
                finalize(() => this.upsertScheduleLoadingSignal.set(false)),
            )
            .subscribe();
    }

    refreshScheduleDeadLetters(): void {
        this.scheduleDeadLettersResource.reload();
    }

    replayScheduleDeadLetter(deadLetterId: number): void {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.scheduleDeadLettersErrorSignal.set('TenantId is not set — select a tenant to replay dead letters.');
            return;
        }
        if (!environment.trim()) {
            this.scheduleDeadLettersErrorSignal.set('Environment is not set — select an environment to replay dead letters.');
            return;
        }
        if (!Number.isFinite(deadLetterId)) {
            this.scheduleDeadLettersErrorSignal.set('Dead letter id is invalid.');
            return;
        }

        this.api
            .replayTenantScheduleDeadLetter(
                { tenantId, environment, deadLetterId },
                this.tenantContext.createRequestOptions('schedules.replay-dead-letter', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.refreshScheduleDeadLetters();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to replay schedule dead letter', error);
                    this.scheduleDeadLettersErrorSignal.set('Unable to replay schedule dead letter.');
                    return EMPTY;
                }),
            )
            .subscribe();
    }

    private hydrate(response: ScheduleListResponse): void {
        this.schedulesSignal.set(response.items);
        this.lastUpdatedSignal.set(response.updatedAt);
    }
}

function normalizeScheduleDeadLettersResponse(value: unknown): ReadonlyArray<ScheduleDeadLetterView> {
    if (!Array.isArray(value)) {
        return [];
    }

    const entries: ScheduleDeadLetterView[] = [];
    for (const [index, item] of value.entries()) {
        if (!item || typeof item !== 'object') {
            continue;
        }
        const record = item as Record<string, unknown>;
        const idRaw = record['id'] ?? record['deadLetterId'] ?? index;
        const id = typeof idRaw === 'number' ? idRaw : Number(idRaw);
        if (!Number.isFinite(id)) {
            continue;
        }
        const triggerId = typeof record['triggerId'] === 'string' ? record['triggerId'] : undefined;
        const jobKey = typeof record['jobKey'] === 'string' ? record['jobKey'] : undefined;
        const occurredAtUtc =
            typeof record['occurredAtUtc'] === 'string'
                ? record['occurredAtUtc']
                : typeof record['timestampUtc'] === 'string'
                    ? record['timestampUtc']
                    : undefined;
        const detail =
            typeof record['detail'] === 'string'
                ? record['detail']
                : typeof record['message'] === 'string'
                    ? record['message']
                    : undefined;

        entries.push({ id, triggerId, jobKey, occurredAtUtc, detail });
    }

    return entries;
}

function createFallbackResponse(): ScheduleListResponse {
    const items = createFallbackSchedules();
    return scheduleListResponseSchema.parse({
        items,
        total: items.length,
        updatedAt: nowIso(),
    });
}

function createEmptyResponse(): ScheduleListResponse {
    return scheduleListResponseSchema.parse({
        items: [],
        total: 0,
        updatedAt: nowIso(),
    });
}

function createFallbackSchedules(): ReadonlyArray<ScheduleSummary> {
    const now = nowMs();
    return [
        {
            id: '8f2059c8-6fb9-4a8f-8d3f-a5b1a7bd81c2',
            name: 'Nightly billing sweep',
            tenant: 'cron-lab',
            cron: '0 2 * * *',
            timezone: 'UTC',
            owner: 'billing@croniq.dev',
            state: 'active',
            nextFire: isoFromEpochMs(now + 1000 * 60 * 90),
            lastDurationMs: 1850,
            alerts: 0,
            tags: ['critical'],
        },
        {
            id: '8d553f98-16fe-4d27-9cf4-e1b4a4250df9',
            name: 'Webhook retry coordinator',
            tenant: 'cron-lab',
            cron: '*/5 * * * *',
            timezone: 'UTC',
            owner: 'hooks@croniq.dev',
            state: 'degraded',
            nextFire: isoFromEpochMs(now + 1000 * 60 * 5),
            lastDurationMs: 5320,
            alerts: 3,
            tags: ['webhooks'],
        },
        {
            id: 'ff0e71b5-5c3c-436b-9116-5e7bbf5e3a6e',
            name: 'Tenant usage snapshot',
            tenant: 'northwind',
            cron: '15 * * * *',
            timezone: 'America/Chicago',
            owner: 'ops@croniq.dev',
            state: 'active',
            nextFire: isoFromEpochMs(now + 1000 * 60 * 15),
            lastDurationMs: 2440,
            alerts: 1,
            tags: ['usage'],
        },
        {
            id: 'be821a56-6630-4b43-9c8a-cc1c8fa4b924',
            name: 'Legacy policy exporter',
            tenant: 'legacy-east',
            cron: '0 */6 * * *',
            timezone: 'UTC',
            owner: 'migrations@croniq.dev',
            state: 'paused',
            nextFire: isoFromEpochMs(now + 1000 * 60 * 60 * 6),
            lastDurationMs: 8840,
            alerts: 0,
            tags: ['migration'],
        },
    ];
}

function normalizeScheduleDetail(value: unknown, fallbackId: string): ScheduleDetail | null {
    if (typeof value !== 'object' || value === null) {
        return { triggerId: fallbackId };
    }

    const record = value as Record<string, unknown>;
    const triggerIdRaw =
        typeof record['triggerId'] === 'string'
            ? record['triggerId']
            : typeof record['id'] === 'string'
                ? record['id']
                : fallbackId;
    const triggerId = triggerIdRaw.trim() || fallbackId;

    const name = typeof record['name'] === 'string' ? record['name'].trim() : undefined;
    const cron = typeof record['cron'] === 'string' ? record['cron'].trim() : undefined;
    const timezone = typeof record['timezone'] === 'string' ? record['timezone'].trim() : undefined;
    const state = typeof record['state'] === 'string' ? record['state'].trim() : undefined;

    const jobKey = typeof record['jobKey'] === 'string' ? record['jobKey'].trim() : undefined;
    const cronExpressionRaw =
        typeof record['cronExpression'] === 'string'
            ? record['cronExpression']
            : typeof record['cron'] === 'string'
                ? record['cron']
                : undefined;
    const cronExpression = cronExpressionRaw?.trim() || undefined;
    const enabled = typeof record['enabled'] === 'boolean' ? record['enabled'] : undefined;
    const startAtUtc = typeof record['startAtUtc'] === 'string' ? record['startAtUtc'].trim() : undefined;
    const endAtUtc = typeof record['endAtUtc'] === 'string' ? record['endAtUtc'].trim() : undefined;
    const description = typeof record['description'] === 'string' ? record['description'].trim() : undefined;

    return {
        triggerId,
        jobKey: jobKey || undefined,
        cronExpression: cronExpression || undefined,
        enabled,
        startAtUtc: startAtUtc || undefined,
        endAtUtc: endAtUtc || undefined,
        description: description || undefined,
        name: name || undefined,
        cron: cron || undefined,
        timezone: timezone || undefined,
        state: state || undefined,
    };
}
