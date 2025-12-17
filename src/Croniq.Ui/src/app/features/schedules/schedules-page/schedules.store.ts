import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs } from '@core/time/clock';
import { ScheduleListResponse, ScheduleSummary, scheduleListResponseSchema } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';

export type ScheduleDetail = {
    triggerId: string;
    name?: string;
    cron?: string;
    timezone?: string;
    state?: string;
};

@Injectable({ providedIn: 'root' })
export class SchedulesStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly schedulesSignal = signal<ReadonlyArray<ScheduleSummary>>([]);
    private readonly lastUpdatedSignal = signal<string>(nowIso());
    readonly loading = signal(true);
    readonly error = signal<string | null>(null);

    private readonly scheduleDetailSignal = signal<ScheduleDetail | null>(null);
    private readonly scheduleDetailLoadingSignal = signal(false);
    private readonly scheduleDetailErrorSignal = signal<string | null>(null);
    private readonly deleteScheduleLoadingSignal = signal(false);
    private readonly deleteScheduleErrorSignal = signal<string | null>(null);

    readonly schedules = this.schedulesSignal.asReadonly();
    readonly lastUpdated = this.lastUpdatedSignal.asReadonly();

    readonly scheduleDetail = this.scheduleDetailSignal.asReadonly();
    readonly scheduleDetailLoading = this.scheduleDetailLoadingSignal.asReadonly();
    readonly scheduleDetailError = this.scheduleDetailErrorSignal.asReadonly();
    readonly deleteScheduleLoading = this.deleteScheduleLoadingSignal.asReadonly();
    readonly deleteScheduleError = this.deleteScheduleErrorSignal.asReadonly();

    constructor() {
        this.hydrate(createFallbackResponse());
        queueMicrotask(() => {
            void this.refresh();
        });
    }

    async refresh(): Promise<void> {
        this.loading.set(true);
        this.error.set(null);
        try {
            const requestOptions = this.tenantContext.createRequestOptions('schedules.refresh');
            const response = await this.api.getSchedules(
                { tenantId: this.tenantContext.snapshot().tenantId },
                requestOptions
            );
            this.hydrate(response);
        } catch (error) {
            console.error('Failed to load schedules', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing schedules permissions for this tenant.',
            });
            if (authFailure) {
                this.error.set(authFailure.message);
                this.schedulesSignal.set([]);
                this.lastUpdatedSignal.set(nowIso());
                return;
            }

            this.error.set('Unable to load schedules from API — showing fallback data.');
            if (this.schedulesSignal().length === 0) {
                this.hydrate(createFallbackResponse());
            }
        } finally {
            this.loading.set(false);
        }
    }

    async refreshScheduleDetail(triggerId: string): Promise<void> {
        const trimmedId = triggerId.trim();
        if (!trimmedId) {
            this.scheduleDetailErrorSignal.set('Trigger id is required to load schedule detail.');
            this.scheduleDetailSignal.set(null);
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.scheduleDetailErrorSignal.set('TenantId is not set — select a tenant to load schedule detail.');
            this.scheduleDetailSignal.set(null);
            return;
        }
        if (!environment.trim()) {
            this.scheduleDetailErrorSignal.set('Environment is not set — select an environment to load schedule detail.');
            this.scheduleDetailSignal.set(null);
            return;
        }

        this.scheduleDetailLoadingSignal.set(true);
        this.scheduleDetailErrorSignal.set(null);
        try {
            const response = await this.api.getSchedule(
                { tenantId, environment, triggerId: trimmedId },
                this.tenantContext.createRequestOptions('schedules.get', {
                    tenantId,
                    environment,
                }),
            );
            this.scheduleDetailSignal.set(normalizeScheduleDetail(response, trimmedId));
        } catch (error) {
            console.error('Failed to load schedule detail', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing schedules permissions for this tenant.',
            });
            if (authFailure) {
                this.scheduleDetailErrorSignal.set(authFailure.message);
                this.scheduleDetailSignal.set(null);
                return;
            }
            if (error instanceof HttpErrorResponse && error.status === 404) {
                this.scheduleDetailErrorSignal.set('Schedule not found (404) — verify the trigger id.');
                this.scheduleDetailSignal.set(null);
                return;
            }
            this.scheduleDetailErrorSignal.set('Unable to load schedule detail from API.');
            this.scheduleDetailSignal.set(null);
        } finally {
            this.scheduleDetailLoadingSignal.set(false);
        }
    }

    async deleteSchedule(triggerId: string): Promise<void> {
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
        try {
            await this.api.deleteSchedule(
                { tenantId, environment, triggerId: trimmedId },
                this.tenantContext.createRequestOptions('schedules.delete', {
                    tenantId,
                    environment,
                }),
            );

            const current = this.scheduleDetailSignal();
            if (current?.triggerId === trimmedId) {
                this.scheduleDetailSignal.set(null);
            }
            this.schedulesSignal.set(this.schedulesSignal().filter((schedule) => schedule.id !== trimmedId));
            void this.refresh();
        } catch (error) {
            console.error('Failed to delete schedule', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing schedules permissions for this tenant.',
            });
            if (authFailure) {
                this.deleteScheduleErrorSignal.set(authFailure.message);
                return;
            }
            if (error instanceof HttpErrorResponse && error.status === 404) {
                this.deleteScheduleErrorSignal.set('Schedule not found (404) — it may have already been deleted.');
                return;
            }
            this.deleteScheduleErrorSignal.set('Unable to delete schedule via API.');
        } finally {
            this.deleteScheduleLoadingSignal.set(false);
        }
    }

    private hydrate(response: ScheduleListResponse): void {
        this.schedulesSignal.set(response.items);
        this.lastUpdatedSignal.set(response.updatedAt);
    }
}

function createFallbackResponse(): ScheduleListResponse {
    const items = createFallbackSchedules();
    return scheduleListResponseSchema.parse({
        items,
        total: items.length,
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

    return {
        triggerId,
        name: name || undefined,
        cron: cron || undefined,
        timezone: timezone || undefined,
        state: state || undefined,
    };
}
