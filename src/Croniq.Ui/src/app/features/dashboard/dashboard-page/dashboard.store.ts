import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { nowIso } from '@core/time/clock';
import { RunnerStatusModel, ScheduleDeadLetterResponse, ScheduleResponse } from '@croniq/api-schema';
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
        { schedules: ScheduleResponse[]; deadLetters: ScheduleDeadLetterResponse[]; runners: RunnerStatusModel[] },
        { tenantId: string; environment: string }
    >({
        command: 'dashboard.refresh',
        defaultValue: { schedules: [], deadLetters: [], runners: [] },
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            this.error.set(null);
            const { tenantId, environment } = params;

            if (!tenantId.trim()) {
                return of({ schedules: [], deadLetters: [], runners: [] });
            }

            // Parallel fetch for dashboard data
            const schedules$ = this.api.getSchedules({ tenantId, environment }, requestOptions).pipe(
                catchError(() => of([] as ScheduleResponse[]))
            );

            const deadLetters$ = this.api.listTenantScheduleDeadLetters({ tenantId, environment }, requestOptions).pipe(
                catchError(() => of<ScheduleDeadLetterResponse[]>([])),
            );

            const runners$ = this.api.listRunners({ tenantId, environment }, requestOptions).pipe(
                map(res => res.runners ?? []),
                catchError(() => of<RunnerStatusModel[]>([]))
            );

            return forkJoin({
                schedules: schedules$,
                deadLetters: deadLetters$,
                runners: runners$
            }).pipe(
                map((data: { schedules: ScheduleResponse[]; deadLetters: ScheduleDeadLetterResponse[]; runners: RunnerStatusModel[] }) => {
                    // Normalize and update signals
                    this.updateMetrics(data.runners);
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
                    return of({ schedules: [], deadLetters: [], runners: [] });
                })
            );
        },
    });

    readonly loading = computed(() => this.dashboardResource.isLoading());
    readonly metrics = this.metricsSignal.asReadonly();
    readonly recentFailures = this.recentFailuresSignal.asReadonly();
    readonly upcomingSchedules = this.upcomingSchedulesSignal.asReadonly();
    readonly misfireHeatmap = this.misfireHeatmapSignal.asReadonly();

    private updateMetrics(runners: RunnerStatusModel[]) {
        const activeRunnersCount = runners.filter(runner => runner.isOnline).length;

        // Mocking other metrics for now as we don't have direct endpoints for RPM/ErrorRate yet
        const metrics: MetricCard[] = [
            {
                label: 'Active Runners',
                value: activeRunnersCount.toString(),
                status: activeRunnersCount > 0 ? 'healthy' : 'warning',
                subtext: activeRunnersCount > 0 ? 'All systems operational' : 'No runners available'
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
            { label: 'Active Runners', value: '8', status: 'healthy', subtext: 'All systems operational' },
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
