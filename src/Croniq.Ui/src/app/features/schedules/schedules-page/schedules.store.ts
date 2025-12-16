import { Injectable, inject, signal } from '@angular/core';

import { ScheduleListResponse, ScheduleSummary, scheduleListResponseSchema } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';

import { authFailureFromError } from '../../../core/auth/auth-failure';
import { TenantContextService } from '../../../core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs } from '../../../core/time/clock';

@Injectable({ providedIn: 'root' })
export class SchedulesStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly schedulesSignal = signal<ReadonlyArray<ScheduleSummary>>([]);
    private readonly lastUpdatedSignal = signal<string>(nowIso());
    readonly loading = signal(true);
    readonly error = signal<string | null>(null);

    readonly schedules = this.schedulesSignal.asReadonly();
    readonly lastUpdated = this.lastUpdatedSignal.asReadonly();

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
