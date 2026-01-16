import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { epochMsFromIso, nowIso, nowMs, tryIsoFromUnknown, utcHourFromEpochMs } from '@core/time/clock';
import { ScheduleDeadLetterResponse, ScheduleForecastResponse, ScheduleResponse } from '@croniq/api-schema';
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

export type ScheduleForecastBucketView = {
    index: number;
    startAtUtc: string;
    endAtUtc: string;
    count: number;
    heightPercent: number;
    label: string;
    showLabel: boolean;
};

export type ScheduleForecastSummaryView = {
    windowMinutes: number;
    count: number;
    label: string;
};

export type ScheduleForecastView = {
    windowMinutes: number;
    bucketMinutes: number;
    rangeLabel: string;
    buckets: ReadonlyArray<ScheduleForecastBucketView>;
    summaries: ReadonlyArray<ScheduleForecastSummaryView>;
    totalCount: number;
    maxBucketCount: number;
    hasData: boolean;
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
const EMPTY_FORECAST: ScheduleForecastView = {
    windowMinutes: 60,
    bucketMinutes: 5,
    rangeLabel: 'Unavailable',
    buckets: [],
    summaries: [],
    totalCount: 0,
    maxBucketCount: 0,
    hasData: false,
};
const DAY_MS = 24 * 60 * 60 * 1000;
const FORECAST_LABEL_MINUTES = 15;

const resolveForecastIso = (value: string | null | undefined, fallback: string): string =>
    tryIsoFromUnknown(value) ?? fallback;

const resolveForecastWindowMinutes = (startUtc: string, endUtc: string): number => {
    const startMs = epochMsFromIso(startUtc);
    const endMs = epochMsFromIso(endUtc);
    if (startMs === null || endMs === null) {
        return EMPTY_FORECAST.windowMinutes;
    }
    const diffMinutes = Math.round((endMs - startMs) / 60000);
    return diffMinutes > 0 ? diffMinutes : EMPTY_FORECAST.windowMinutes;
};

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

type ScheduleSummary = {
    total: number;
    active: number;
    paused: number;
};

type DeadLetterSummary = {
    total: number;
    recent: number;
    heatmap: ReadonlyArray<number>;
};

const summarizeSchedules = (entries: ReadonlyArray<ScheduleResponse>): ScheduleSummary => {
    const total = entries.length;
    const active = entries.reduce((count, entry) => count + (entry.enabled ? 1 : 0), 0);
    return {
        total,
        active,
        paused: Math.max(0, total - active),
    };
};

const formatTimeLabel = (iso: string): string => {
    const date = new Date(iso);
    const hours = String(date.getHours()).padStart(2, '0');
    const minutes = String(date.getMinutes()).padStart(2, '0');
    return `${hours}:${minutes}`;
};

const formatSummaryLabel = (minutes: number): string => {
    if (minutes < 60) {
        return `Next ${minutes}m`;
    }

    if (minutes % 60 === 0) {
        const hours = minutes / 60;
        return `Next ${hours}h`;
    }

    return `Next ${minutes}m`;
};

const resolveDeadLetterTimestamp = (entry: ScheduleDeadLetterResponse): number | null => {
    const raw = entry.fireAtUtc ?? entry.createdAtUtc ?? null;
    if (typeof raw !== 'string') {
        return null;
    }
    const trimmed = raw.trim();
    if (!trimmed) {
        return null;
    }
    return epochMsFromIso(trimmed);
};

const summarizeDeadLetters = (
    entries: ReadonlyArray<ScheduleDeadLetterResponse>,
    nowEpochMs: number,
): DeadLetterSummary => {
    const total = entries.length;
    const cutoff = nowEpochMs - DAY_MS;
    const buckets = new Array<number>(24).fill(0);
    let recent = 0;
    let bucketed = false;

    entries.forEach((entry) => {
        const timestamp = resolveDeadLetterTimestamp(entry);
        if (timestamp === null || timestamp < cutoff) {
            return;
        }
        const hour = utcHourFromEpochMs(timestamp);
        buckets[hour] += 1;
        recent += 1;
        bucketed = true;
    });

    return {
        total,
        recent,
        heatmap: bucketed ? buckets : [],
    };
};

const formatPresenceSubtext = (summary: PresenceSummary, emptyLabel: string): string =>
    summary.total > 0 ? `${summary.online}/${summary.total} online` : emptyLabel;

const formatWebhookSubtext = (summary: WebhookSummary): string =>
    summary.total > 0 ? `${summary.enabled}/${summary.total} enabled` : 'No webhooks configured';

const formatScheduleSubtext = (summary: ScheduleSummary): string =>
    summary.total > 0
        ? `${summary.active} active / ${summary.paused} paused`
        : 'No schedules configured';

const formatDeadLetterSubtext = (summary: DeadLetterSummary): string =>
    summary.total > 0 ? `${summary.total} total` : 'No dead letters recorded';

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

const statusFromDeadLetters = (count: number): MetricCard['status'] => {
    if (count === 0) {
        return 'healthy';
    }
    if (count <= 3) {
        return 'warning';
    }
    return 'critical';
};

@Injectable()
export class DashboardStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly metricsSignal = signal<ReadonlyArray<MetricCard>>([]);
    private readonly recentFailuresSignal = signal<ReadonlyArray<DeadLetter>>([]);
    private readonly scheduleForecastSignal = signal<ScheduleForecastView>(EMPTY_FORECAST);
    private readonly misfireHeatmapSignal = signal<ReadonlyArray<number>>([]); // 24h counters

    readonly error = signal<string | null>(null);

    private readonly dashboardResource = tenantRxResource<
        {
            schedules: ScheduleResponse[];
            deadLetters: ScheduleDeadLetterResponse[];
            runnerSummary: PresenceSummary;
            workerSummary: PresenceSummary;
            webhookSummary: WebhookSummary;
            forecast: ScheduleForecastResponse | null;
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
            forecast: null,
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
                    forecast: null,
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

            const forecast$ = this.api
                .getScheduleForecast(
                    {
                        tenantId,
                        environment,
                        windowMinutes: 60,
                        bucketMinutes: 5,
                        summaryMinutes: '5,15,60',
                    },
                    requestOptions,
                )
                .pipe(catchError(() => of(null)));

            return forkJoin({
                schedules: schedules$,
                deadLetters: deadLetters$,
                runnerSummary: runners$,
                workerSummary: workers$,
                webhookSummary: webhooks$,
                forecast: forecast$,
            }).pipe(
                map((data: {
                    schedules: ScheduleResponse[];
                    deadLetters: ScheduleDeadLetterResponse[];
                    runnerSummary: PresenceSummary;
                    workerSummary: PresenceSummary;
                    webhookSummary: WebhookSummary;
                    forecast: ScheduleForecastResponse | null;
                }) => {
                    const nowEpochMs = nowMs();
                    const scheduleSummary = summarizeSchedules(data.schedules);
                    const deadLetterSummary = summarizeDeadLetters(data.deadLetters, nowEpochMs);

                    // Normalize and update signals
                    this.updateMetrics(
                        scheduleSummary,
                        deadLetterSummary,
                        data.runnerSummary,
                        data.workerSummary,
                        data.webhookSummary,
                    );
                    this.updateFailures(data.deadLetters);
                    this.updateForecast(data.forecast);
                    this.misfireHeatmapSignal.set(deadLetterSummary.heatmap);
                    return data;
                }),
                catchError((error) => {
                    console.error('Failed to load dashboard data', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - missing dashboard permissions.',
                    });
                    if (authFailure) {
                        this.error.set(authFailure.message);
                    } else {
                        this.error.set('Unable to load dashboard data.');
                    }
                    return of({
                        schedules: [],
                        deadLetters: [],
                        runnerSummary: EMPTY_PRESENCE_SUMMARY,
                        workerSummary: EMPTY_PRESENCE_SUMMARY,
                        webhookSummary: EMPTY_WEBHOOK_SUMMARY,
                        forecast: null,
                    });
                })
            );
        },
    });

    readonly loading = computed(() => this.dashboardResource.isLoading());
    readonly metrics = this.metricsSignal.asReadonly();
    readonly recentFailures = this.recentFailuresSignal.asReadonly();
    readonly scheduleForecast = this.scheduleForecastSignal.asReadonly();
    readonly misfireHeatmap = this.misfireHeatmapSignal.asReadonly();

    private updateMetrics(
        scheduleSummary: ScheduleSummary,
        deadLetterSummary: DeadLetterSummary,
        runnerSummary: PresenceSummary,
        workerSummary: PresenceSummary,
        webhookSummary: WebhookSummary,
    ) {
        const activeSchedules = scheduleSummary.active;
        const hasSchedules = scheduleSummary.total > 0;

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
                label: 'Schedules',
                value: scheduleSummary.total.toString(),
                status: hasSchedules ? 'healthy' : 'warning',
                subtext: formatScheduleSubtext(scheduleSummary),
            },
            {
                label: 'Active Schedules',
                value: activeSchedules.toString(),
                status: activeSchedules > 0 ? 'healthy' : 'warning',
                subtext: hasSchedules ? `${scheduleSummary.paused} paused` : 'No schedules configured',
            },
            {
                label: 'Dead Letters (24h)',
                value: deadLetterSummary.recent.toString(),
                status: statusFromDeadLetters(deadLetterSummary.recent),
                subtext: formatDeadLetterSubtext(deadLetterSummary),
            },
        ];
        this.metricsSignal.set(metrics);
    }

    private updateForecast(forecast: ScheduleForecastResponse | null) {
        if (!forecast || !Array.isArray(forecast.buckets) || forecast.buckets.length === 0) {
            this.scheduleForecastSignal.set(EMPTY_FORECAST);
            return;
        }

        const nowUtc = nowIso();
        const windowStartUtc = resolveForecastIso(forecast.windowStartUtc, nowUtc);
        const windowEndUtc = resolveForecastIso(forecast.windowEndUtc, windowStartUtc);
        const windowMinutes = resolveForecastWindowMinutes(windowStartUtc, windowEndUtc);

        const bucketMinutes = forecast.bucketMinutes ?? EMPTY_FORECAST.bucketMinutes;
        const maxBucketCount = forecast.buckets.reduce((max, bucket) => Math.max(max, bucket.count ?? 0), 0);
        const bucketLabelStride = Math.max(1, Math.floor(FORECAST_LABEL_MINUTES / bucketMinutes));
        const buckets = forecast.buckets.map((bucket, index) => {
            const count = bucket.count ?? 0;
            const heightPercent = maxBucketCount > 0
                ? Math.max(8, Math.round((count / maxBucketCount) * 100))
                : 0;
            const startAtUtc = resolveForecastIso(bucket.startAtUtc, windowStartUtc);
            const endAtUtc = resolveForecastIso(bucket.endAtUtc, windowEndUtc);
            return {
                index,
                startAtUtc,
                endAtUtc,
                count,
                heightPercent,
                label: formatTimeLabel(startAtUtc),
                showLabel: index % bucketLabelStride === 0,
            };
        });

        const summaries = Array.isArray(forecast.summaries)
            ? forecast.summaries.map((summary) => ({
                windowMinutes: summary.windowMinutes ?? 0,
                count: summary.count ?? 0,
                label: formatSummaryLabel(summary.windowMinutes ?? 0),
            }))
            : [];

        const totalCount = buckets.reduce((sum, bucket) => sum + bucket.count, 0);
        const rangeLabel = `${formatTimeLabel(windowStartUtc)} - ${formatTimeLabel(windowEndUtc)}`;

        this.scheduleForecastSignal.set({
            windowMinutes,
            bucketMinutes,
            rangeLabel,
            buckets,
            summaries,
            totalCount,
            maxBucketCount,
            hasData: buckets.length > 0,
        });
    }

    private updateFailures(deadLetters: ScheduleDeadLetterResponse[]) {
        if (!Array.isArray(deadLetters)) {
            this.recentFailuresSignal.set([]);
            return;
        }

        const failures = deadLetters
            .map((deadLetter, index) => {
                const rawTime = deadLetter.createdAtUtc ?? deadLetter.fireAtUtc ?? null;
                const time = tryIsoFromUnknown(rawTime) ?? nowIso();
                return {
                    id: deadLetter.id ?? index,
                    jobKey: deadLetter.jobKey || deadLetter.triggerId || 'Unknown',
                    reason: deadLetter.reason || 'Unknown Error',
                    time,
                };
            })
            .sort((a, b) => (epochMsFromIso(b.time) ?? 0) - (epochMsFromIso(a.time) ?? 0))
            .slice(0, 5);

        this.recentFailuresSignal.set(failures);
    }

}
