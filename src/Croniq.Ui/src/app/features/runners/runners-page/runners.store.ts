import { Injectable, computed, inject } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { ExecutionResponse, JobResponse, RunnerListResponse, RunnerStatusModel } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { catchError, map, of } from 'rxjs';

export interface Runner {
    id: string;
    hostname: string;
    status: 'Online' | 'Offline' | 'Draining';
    lastHeartbeatAt: string;
    activeJobs: number;
    capacity: number;
    tags: string[];
    capabilities: string[];
    runnerInstanceId?: string;
    runnerInstanceLabel: string;
    transportState?: string;
    transportLabel: string;
    allowTestExecutions?: boolean;
    allowTestLabel: string;
    maxInflight?: number;
    maxInflightLabel: string;
    draining: boolean;
    drainLabel: string;
    loadPercent: number;
    loadLabel: string;
    recentJobs: ReadonlyArray<string>;
    assignedJobs: ReadonlyArray<string>;
}

interface RunnerMetadata {
    hostname?: string;
    tags?: string[];
    capacity?: number;
    activeJobs?: number;
    runnerInstanceId?: string;
    transportState?: string;
    allowTestExecutions?: boolean;
    maxInflight?: number;
    draining?: boolean;
    capabilities?: string[];
}

const DEFAULT_LOAD_LABEL = 'n/a';
const DEFAULT_VALUE_LABEL = '--';
const MAX_RECENT_JOBS = 4;

const parseRunnerMetadata = (metadataJson?: string | null): RunnerMetadata => {
    if (!metadataJson) {
        return {};
    }

    try {
        const parsed: unknown = JSON.parse(metadataJson);
        if (!parsed || typeof parsed !== 'object') {
            return {};
        }

        const record = parsed as Record<string, unknown>;
        const hostnameValue = record['hostname'];
        const tagsValue = record['tags'];
        const capacityValue = record['capacity'];
        const activeJobsValue = record['activeJobs'];
        const runnerInstanceValue = record['runnerInstanceId'];
        const transportStateValue = record['transportState'];
        const allowTestValue = record['allowTestExecutions'];
        const maxInflightValue = record['maxInflight'];
        const drainingValue = record['draining'];
        const drainStatusValue = record['drainStatus'];
        const capabilitiesValue = record['capabilities'];

        const hostname = typeof hostnameValue === 'string' ? hostnameValue : undefined;
        const tags = Array.isArray(tagsValue)
            ? tagsValue.filter((tag): tag is string => typeof tag === 'string')
            : undefined;
        const capacity = typeof capacityValue === 'number' ? capacityValue : undefined;
        const activeJobs = typeof activeJobsValue === 'number' ? activeJobsValue : undefined;
        const runnerInstanceId = typeof runnerInstanceValue === 'string' ? runnerInstanceValue : undefined;
        const transportState = typeof transportStateValue === 'string' ? transportStateValue : undefined;
        const allowTestExecutions = typeof allowTestValue === 'boolean' ? allowTestValue : undefined;
        const maxInflight = typeof maxInflightValue === 'number' ? maxInflightValue : undefined;
        const capabilities = Array.isArray(capabilitiesValue)
            ? capabilitiesValue.filter((tag): tag is string => typeof tag === 'string')
            : undefined;
        const draining = typeof drainingValue === 'boolean'
            ? drainingValue
            : typeof drainStatusValue === 'string'
                ? drainStatusValue.toLowerCase() === 'draining'
                : typeof drainStatusValue === 'boolean'
                    ? drainStatusValue
                    : undefined;

        return {
            hostname,
            tags,
            capacity,
            activeJobs,
            runnerInstanceId,
            transportState,
            allowTestExecutions,
            maxInflight,
            draining,
            capabilities,
        };
    } catch {
        return {};
    }
};

const buildRunnerTags = (metadata: RunnerMetadata): string[] => {
    const tags = metadata.tags ?? [];
    const capabilities = metadata.capabilities ?? [];
    if (tags.length === 0 && capabilities.length === 0) {
        return [];
    }
    return Array.from(new Set([...tags, ...capabilities])).sort((a, b) => a.localeCompare(b));
};

const toTransportLabel = (transport?: string): string => {
    if (!transport) {
        return DEFAULT_VALUE_LABEL;
    }
    const normalized = transport.trim().toLowerCase();
    if (normalized === 'grpc') {
        return 'gRPC';
    }
    if (normalized === 'polling') {
        return 'Polling';
    }
    return transport;
};

const toAllowTestLabel = (allowTest?: boolean): string => {
    if (allowTest === undefined) {
        return DEFAULT_VALUE_LABEL;
    }
    return allowTest ? 'Allowed' : 'Blocked';
};

const toMaxInflightLabel = (maxInflight?: number): string => {
    if (!maxInflight || maxInflight <= 0) {
        return DEFAULT_VALUE_LABEL;
    }
    return maxInflight.toString();
};

const mapRunnerStatus = (runner: RunnerStatusModel): Runner => {
    const metadata = parseRunnerMetadata(runner.metadataJson);
    const capacity = metadata.capacity ?? 0;
    const activeJobs = metadata.activeJobs ?? 0;
    const loadPercent = capacity > 0 ? Math.min(100, (activeJobs / capacity) * 100) : 0;
    const loadLabel = capacity > 0 ? `${activeJobs}/${capacity}` : DEFAULT_LOAD_LABEL;
    const tags = buildRunnerTags(metadata);
    const draining = metadata.draining === true;
    const status = runner.isOnline ? (draining ? 'Draining' : 'Online') : 'Offline';
    const drainLabel = runner.isOnline
        ? (draining ? 'Draining' : 'Accepting work')
        : 'Offline';

    return {
        id: runner.runnerId,
        hostname: metadata.hostname ?? runner.runnerId,
        status,
        lastHeartbeatAt: runner.lastSeenAtUtc ?? '',
        activeJobs,
        capacity,
        tags,
        capabilities: metadata.capabilities ?? [],
        runnerInstanceId: metadata.runnerInstanceId,
        runnerInstanceLabel: metadata.runnerInstanceId ?? DEFAULT_VALUE_LABEL,
        transportState: metadata.transportState,
        transportLabel: toTransportLabel(metadata.transportState),
        allowTestExecutions: metadata.allowTestExecutions,
        allowTestLabel: toAllowTestLabel(metadata.allowTestExecutions),
        maxInflight: metadata.maxInflight,
        maxInflightLabel: toMaxInflightLabel(metadata.maxInflight),
        draining,
        drainLabel,
        loadPercent,
        loadLabel,
        recentJobs: [],
        assignedJobs: [],
    };
}

const toEpochMs = (value?: string | null): number => {
    if (!value) {
        return 0;
    }

    const parsed = Date.parse(value);
    return Number.isFinite(parsed) ? parsed : 0;
};

const buildRecentJobsByRunner = (executions: ExecutionResponse[]): Map<string, string[]> => {
    const sorted = [...executions].sort((a, b) => toEpochMs(b.startedAtUtc) - toEpochMs(a.startedAtUtc));
    const results = new Map<string, string[]>();

    for (const exec of sorted) {
        const runnerId = typeof exec.instanceId === 'string' ? exec.instanceId.trim() : '';
        const jobKey = typeof exec.jobKey === 'string' ? exec.jobKey.trim() : '';
        if (!runnerId || !jobKey) {
            continue;
        }

        const current = results.get(runnerId) ?? [];
        if (current.includes(jobKey)) {
            continue;
        }

        current.push(jobKey);
        results.set(runnerId, current);
    }

    for (const [runnerId, jobs] of results.entries()) {
        if (jobs.length > MAX_RECENT_JOBS) {
            results.set(runnerId, jobs.slice(0, MAX_RECENT_JOBS));
        }
    }

    return results;
};

const buildAssignedJobsByRunner = (jobs: JobResponse[]): Map<string, string[]> => {
    const results = new Map<string, string[]>();

    for (const job of jobs) {
        const runnerId = typeof job.assignedRunnerId === 'string' ? job.assignedRunnerId.trim() : '';
        const jobKey = typeof job.jobKey === 'string' ? job.jobKey.trim() : '';
        if (!runnerId || !jobKey) {
            continue;
        }

        const list = results.get(runnerId) ?? [];
        if (!list.includes(jobKey)) {
            list.push(jobKey);
        }
        results.set(runnerId, list);
    }

    for (const [runnerId, list] of results.entries()) {
        results.set(runnerId, list.sort((a, b) => a.localeCompare(b)));
    }

    return results;
};

@Injectable()
export class RunnersStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    readonly runnersResource = tenantRxResource<Runner[], { tenantId: string; environment: string }>({
        command: 'runners.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const { tenantId, environment } = params;
            if (!tenantId) return of([]);

            return this.api.listRunners({ tenantId, environment, includeOffline: true }, requestOptions).pipe(
                map((response: RunnerListResponse) => (response.runners ?? []).map(mapRunnerStatus)),
                catchError(err => {
                    console.error('Failed to load runners', err);
                    return of<Runner[]>([]);
                })
            );
        }
    });

    readonly executionsResource = tenantRxResource<ExecutionResponse[], { tenantId: string; environment: string }>({
        command: 'executions.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const { tenantId, environment } = params;
            if (!tenantId) {
                return of([] as ExecutionResponse[]);
            }

            return this.api.listExecutions({ tenantId, environment, limit: 200 }, requestOptions).pipe(
                map((response) => (Array.isArray(response) ? response as ExecutionResponse[] : [])),
                catchError((err) => {
                    console.error('Failed to load executions for runner jobs', err);
                    return of([] as ExecutionResponse[]);
                }),
            );
        },
    });

    readonly jobsResource = tenantRxResource<JobResponse[], { tenantId: string; environment: string }>({
        command: 'jobs.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const { tenantId, environment } = params;
            if (!tenantId) {
                return of([] as JobResponse[]);
            }

            return this.api.listJobs({ tenantId, environment }, requestOptions).pipe(
                map((response) => (Array.isArray(response) ? response as JobResponse[] : [])),
                catchError((err) => {
                    console.error('Failed to load assigned jobs', err);
                    return of([] as JobResponse[]);
                }),
            );
        },
    });

    readonly runners = computed(() => {
        const runners = this.runnersResource.value() ?? [];
        const executions = this.executionsResource.value() ?? [];
        const jobs = this.jobsResource.value() ?? [];
        const jobsByRunner = buildRecentJobsByRunner(executions);
        const assignedJobsByRunner = buildAssignedJobsByRunner(jobs);

        return runners.map((runner) => ({
            ...runner,
            recentJobs: jobsByRunner.get(runner.id) ?? [],
            assignedJobs: assignedJobsByRunner.get(runner.id) ?? [],
        }));
    });
    readonly loading = computed(() => this.runnersResource.isLoading());
    readonly error = computed(() => this.runnersResource.error());

    // Metrics
    readonly activeRunnersCount = computed(() => this.runners().filter(r => r.status === 'Online').length);
    readonly totalCapacity = computed(() => this.runners().reduce((acc, r) => acc + (r.capacity || 0), 0));
    readonly busyThreads = computed(() => this.runners().reduce((acc, r) => acc + (r.activeJobs || 0), 0));

    refresh() {
        this.runnersResource.reload();
        this.jobsResource.reload();
    }
}
