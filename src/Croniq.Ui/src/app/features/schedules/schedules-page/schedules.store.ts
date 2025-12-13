import { Injectable, inject, signal } from '@angular/core';

import { ScheduleListResponse, ScheduleSummary, scheduleListResponseSchema } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';

@Injectable({ providedIn: 'root' })
export class SchedulesStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);

    private readonly schedulesSignal = signal<ReadonlyArray<ScheduleSummary>>([]);
    private readonly lastUpdatedSignal = signal<string>(new Date().toISOString());
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
            const response = await this.api.getSchedules();
            this.hydrate(response);
        } catch (error) {
            console.error('Failed to load schedules', error);
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
        updatedAt: new Date().toISOString(),
    });
}

function createFallbackSchedules(): ReadonlyArray<ScheduleSummary> {
    return [
        {
            id: '8f2059c8-6fb9-4a8f-8d3f-a5b1a7bd81c2',
            name: 'Nightly billing sweep',
            tenant: 'cron-lab',
            cron: '0 2 * * *',
            timezone: 'UTC',
            owner: 'billing@croniq.dev',
            state: 'active',
            nextFire: new Date(Date.now() + 1000 * 60 * 90).toISOString(),
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
            nextFire: new Date(Date.now() + 1000 * 60 * 5).toISOString(),
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
            nextFire: new Date(Date.now() + 1000 * 60 * 15).toISOString(),
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
            nextFire: new Date(Date.now() + 1000 * 60 * 60 * 6).toISOString(),
            lastDurationMs: 8840,
            alerts: 0,
            tags: ['migration'],
        },
    ];
}
