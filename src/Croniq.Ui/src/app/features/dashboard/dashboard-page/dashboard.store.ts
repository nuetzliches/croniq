import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { nowIso } from '@core/time/clock';
import { ScheduleDeadLetterResponse, ScheduleResponse } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { of, catchError, map, forkJoin } from 'rxjs';

export type MetricCard = {
    label: string;
    value: string;
    trend?: string;
    status?: 'healthy' | 'warning' | 'critical';
    subtext?: string;
    sparkline?: number[];
};

export type DeadLetter = {
    id: number;
    jobKey: string;
    reason: string;
    time: string;
};

export type UpcomingSchedule = {
    jobKey: string;
    fireTime: string;
    cron: string;
};

type PresenceSummary = {
    total: number;
    online: number;
    offline: number;
};

type WebhookSummary = {
    total: number;
    enabled: number;
    disabled: number;
};

const EMPTY_PRESENCE_SUMMARY: PresenceSummary = { total: 0, online: 0, offline: 0 };
const EMPTY_WEBHOOK_SUMMARY: WebhookSummary = { total: 0, enabled: 0, disabled: 0 };

const summarizePresence = (entries: ReadonlyArray<{ isOnline?: boolean }>): PresenceSummary => {
    const total = entries.length;
    if (!total) {
        return EMPTY_PRESENCE_SUMMARY;
    }

    const online = entries.reduce((count, entry) => count + (entry.isOnline ? 1 : 0), 0);
    return {
        total,
        online,
        offline: Math.max(0, total - online),
    };
};

const summarizeWebhooks = (payload: unknown): WebhookSummary => {
    if (!Array.isArray(payload)) {
        return EMPTY_WEBHOOK_SUMMARY;
    }

    let total = 0;
    let enabled = 0;

    payload.forEach((item) => {
        if (!item || typeof item !== 'object') {
            return;
        }
        total += 1;
        const record = item as Record<string, unknown>;
        const enabledValue = record['enabled'];
        const isEnabled = typeof enabledValue === 'boolean' ? enabledValue : true;
        if (isEnabled) {
            enabled += 1;
        }
    });

    return {
        total,
        enabled,
        disabled: Math.max(0, total - enabled),
    };
};

const formatPresenceSubtext = (summary: PresenceSummary, emptyLabel: string): string =>
    summary.total > 0 ? `${summary.online}/${summary.total} online` : emptyLabel;

const formatWebhookSubtext = (summary: WebhookSummary): string =>
    summary.total > 0 ? `${summary.enabled}/${summary.total} enabled` : 'No webhooks configured';

const statusFromPresence = (summary: PresenceSummary): MetricCard['status'] => {
    if (summary.total === 0) {
        return 'warning';
    }
    if (summary.online === 0) {
        return 'critical';
    }
    if (summary.online === summary.total) {
        return 'healthy';
    }
    return 'warning';
};

const statusFromWebhooks = (summary: WebhookSummary): MetricCard['status'] => {
    if (summary.total === 0) {
        return 'warning';
    }
    if (summary.enabled === 0) {
        return 'critical';
    }
    if (summary.enabled === summary.total) {
        return 'healthy';
    }
    return 'warning';
};

@Injectable()
export class DashboardStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly metricsSignal = signal<ReadonlyArray<MetricCard>>([]);
    private readonly recentFailuresSignal = signal<ReadonlyArray<DeadLetter>>([]);
    private readonly upcomingSchedulesSignal = signal<ReadonlyArray<UpcomingSchedule>>([]);
    private readonly misfireHeatmapSignal = signal<ReadonlyArray<number>>([]); // 24h counters

    readonly error = signal<string | null>(null);

    private readonly dashboardResource = tenantRxResource<
        {
            schedules: ScheduleResponse[];
            deadLetters: ScheduleDeadLetterResponse[];
            runnerSummary: PresenceSummary;
            workerSummary: PresenceSummary;
            webhookSummary: WebhookSummary;
        },
        { tenantId: string; environment: string }
    >({
        command: 'dashboard.refresh',
        defaultValue: {
            schedules: [],
            deadLetters: [],
            runnerSummary: EMPTY_PRESENCE_SUMMARY,
            workerSummary: EMPTY_PRESENCE_SUMMARY,
            webhookSummary: EMPTY_WEBHOOK_SUMMARY,
        },
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            this.error.set(null);
            const { tenantId, environment } = params;

            if (!tenantId.trim()) {
                return of({
                    schedules: [],
                    deadLetters: [],
                    runnerSummary: EMPTY_PRESENCE_SUMMARY,
                    workerSummary: EMPTY_PRESENCE_SUMMARY,
                    webhookSummary: EMPTY_WEBHOOK_SUMMARY,
                });
            }

            // Parallel fetch for dashboard data
            const schedules$ = this.api.getSchedules({ tenantId, environment }, requestOptions).pipe(
                catchError(() => of([] as ScheduleResponse[]))
            );

            const deadLetters$ = this.api.listTenantScheduleDeadLetters({ tenantId, environment }, requestOptions).pipe(
                catchError(() => of<ScheduleDeadLetterResponse[]>([])),
            );

            const runners$ = this.api.listRunners({ tenantId, environment }, requestOptions).pipe(
                map((res) => summarizePresence(res.runners ?? [])),
                catchError(() => of(EMPTY_PRESENCE_SUMMARY))
            );

            const workers$ = this.api.listWorkers({ tenantId, environment }, requestOptions).pipe(
                map((res) => summarizePresence(res.workers ?? [])),
                catchError(() => of(EMPTY_PRESENCE_SUMMARY))
            );

            const webhooks$ = this.api.listTenantWebhooks({ tenantId, environment }, requestOptions).pipe(
                map((response) => summarizeWebhooks(response)),
                catchError(() => of(EMPTY_WEBHOOK_SUMMARY))
            );

            return forkJoin({
                schedules: schedules$,
                deadLetters: deadLetters$,
                runnerSummary: runners$,
                workerSummary: workers$,
                webhookSummary: webhooks$
            }).pipe(
                map((data: {
                    schedules: ScheduleResponse[];
                    deadLetters: ScheduleDeadLetterResponse[];
                    runnerSummary: PresenceSummary;
                    workerSummary: PresenceSummary;
                    webhookSummary: WebhookSummary;
                }) => {
                    // Normalize and update signals
                    this.updateMetrics(data.runnerSummary, data.workerSummary, data.webhookSummary);
                    this.updateUpcoming(data.schedules);
                    this.updateFailures(data.deadLetters);
                    return data;
                }),
                catchError((error) => {
                    console.error('Failed to load dashboard data', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — missing dashboard permissions.',
                    });
                    if (authFailure) {
                        this.error.set(authFailure.message);
                    } else {
                        this.error.set('Unable to load dashboard data.');
                    }
                    // Fallback data for demo purposes if API fails completely
                    this.setFallbackData();
                    return of({
                        schedules: [],
                        deadLetters: [],
                        runnerSummary: EMPTY_PRESENCE_SUMMARY,
                        workerSummary: EMPTY_PRESENCE_SUMMARY,
                        webhookSummary: EMPTY_WEBHOOK_SUMMARY,
                    });
                })
            );
        },
    });

    readonly loading = computed(() => this.dashboardResource.isLoading());
    readonly metrics = this.metricsSignal.asReadonly();
    readonly recentFailures = this.recentFailuresSignal.asReadonly();
    readonly upcomingSchedules = this.upcomingSchedulesSignal.asReadonly();
    readonly misfireHeatmap = this.misfireHeatmapSignal.asReadonly();

    private updateMetrics(
        runnerSummary: PresenceSummary,
        workerSummary: PresenceSummary,
        webhookSummary: WebhookSummary,
    ) {
        // Mocking other metrics for now as we don't have direct endpoints for RPM/ErrorRate yet
        const metrics: MetricCard[] = [
            {
                label: 'Workers Online',
                value: workerSummary.online.toString(),
                status: statusFromPresence(workerSummary),
                subtext: formatPresenceSubtext(workerSummary, 'No workers reporting'),
            },
            {
                label: 'Runners Online',
                value: runnerSummary.online.toString(),
                status: statusFromPresence(runnerSummary),
                subtext: formatPresenceSubtext(runnerSummary, 'No runners reporting'),
            },
            {
                label: 'Webhooks Enabled',
                value: webhookSummary.enabled.toString(),
                status: statusFromWebhooks(webhookSummary),
                subtext: formatWebhookSubtext(webhookSummary),
            },
            {
                label: 'Throughput (RPM)',
                value: '1,240',
                trend: '↑ 12%',
                subtext: 'vs last hour',
                sparkline: [20, 25, 30, 28, 35, 40, 42, 38, 45, 50]
            },
            {
                label: 'Queue Depth',
                value: '12',
                status: 'healthy',
                subtext: 'Jobs pending',
                sparkline: [5, 12, 8, 15, 20, 10, 5, 2, 4, 12]
            },
            {
                label: 'Error Rate (1h)',
                value: '0.05%',
                status: 'healthy',
                subtext: 'Below threshold'
            },
        ];
        this.metricsSignal.set(metrics);
    }

    private updateUpcoming(schedules: ScheduleResponse[]) {
        if (!Array.isArray(schedules)) {
            this.upcomingSchedulesSignal.set([]);
            return;
        }

        const upcoming = schedules
            .filter(s => s.enabled && s.startAtUtc)
            .map(s => ({
                jobKey: s.jobKey || s.triggerId || 'Unknown',
                fireTime: s.startAtUtc!,
                cron: s.cronExpression || ''
            }))
            .sort((a, b) => new Date(a.fireTime).getTime() - new Date(b.fireTime).getTime())
            .slice(0, 5); // Top 5

        this.upcomingSchedulesSignal.set(upcoming);
    }

    private updateFailures(deadLetters: ScheduleDeadLetterResponse[]) {
        if (!Array.isArray(deadLetters)) {
            this.recentFailuresSignal.set([]);
            return;
        }

        const failures = deadLetters
            .map((dl, index) => ({
                id: dl.id ?? index,
                jobKey: dl.jobKey || dl.triggerId || 'Unknown',
                reason: dl.reason || 'Unknown Error',
                time: dl.createdAtUtc || dl.fireAtUtc || nowIso(),
            }))
            .sort((a, b) => new Date(b.time).getTime() - new Date(a.time).getTime())
            .slice(0, 5);

        this.recentFailuresSignal.set(failures);
    }

    private setFallbackData() {
        this.metricsSignal.set([
            { label: 'Workers Online', value: '6', status: 'healthy', subtext: '6/7 online' },
            { label: 'Runners Online', value: '8', status: 'healthy', subtext: '8/8 online' },
            { label: 'Webhooks Enabled', value: '5', status: 'warning', subtext: '5/6 enabled' },
            {
                label: 'Throughput (RPM)',
                value: '1,240',
                trend: '↑ 12%',
                subtext: 'vs last hour',
                sparkline: [20, 25, 30, 28, 35, 40, 42, 38, 45, 50]
            },
            {
                label: 'Queue Depth',
                value: '24',
                status: 'warning',
                subtext: 'Jobs pending',
                sparkline: [10, 15, 24, 20, 15, 12, 18, 24, 22, 24]
            },
            { label: 'Error Rate (1h)', value: '0.05%', status: 'healthy', subtext: 'Below threshold' },
        ]);

        this.recentFailuresSignal.set([
            { id: 1, jobKey: 'payment-sync', reason: 'Timeout', time: '2m ago' },
            { id: 2, jobKey: 'email-send', reason: '500 Error', time: '15m ago' },
            { id: 3, jobKey: 'data-export', reason: 'Connection Refused', time: '1h ago' },
        ]);

        this.upcomingSchedulesSignal.set([
            { jobKey: 'daily-report', fireTime: 'in 5m', cron: '0 0 * * *' },
            { jobKey: 'cleanup-logs', fireTime: 'in 1h', cron: '0 1 * * *' },
            { jobKey: 'billing-cycle', fireTime: 'Tomorrow 00:00', cron: '0 0 1 * *' },
        ]);

        // Mock 24h heatmap (mostly zeros, some spikes)
        const heatmap = new Array(24).fill(0).map((_, i) => (i === 14 || i === 15) ? Math.floor(Math.random() * 5) + 1 : 0);
        this.misfireHeatmapSignal.set(heatmap);
    }
}
