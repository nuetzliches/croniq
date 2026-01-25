import { Injectable, computed, inject } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { WorkerListResponse, WorkerStatusModel } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { catchError, map, of } from 'rxjs';

export interface Worker {
    id: string;
    hostname: string;
    status: 'Online' | 'Offline' | 'Draining';
    dispatchState: 'Connected' | 'Fallback' | 'Unknown';
    dispatchLastConnectedAt?: string;
    dispatchLastFallbackAt?: string;
    lastHeartbeatAt: string;
    activeJobs: number;
    capacity: number;
    tags: string[];
    loadPercent: number;
    loadLabel: string;
}

interface WorkerMetadata {
    hostname?: string;
    tags?: string[];
    capacity?: number;
    activeJobs?: number;
    dispatch?: {
        grpcConnected?: boolean;
        lastConnectedAtUtc?: string;
        lastFallbackAtUtc?: string;
    };
}

const DEFAULT_LOAD_LABEL = 'n/a';

const parseWorkerMetadata = (metadataJson?: string | null): WorkerMetadata => {
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
        const dispatchValue = record['dispatch'];

        const hostname = typeof hostnameValue === 'string' ? hostnameValue : undefined;
        const tags = Array.isArray(tagsValue)
            ? tagsValue.filter((tag): tag is string => typeof tag === 'string')
            : undefined;
        const capacity = typeof capacityValue === 'number' ? capacityValue : undefined;
        const activeJobs = typeof activeJobsValue === 'number' ? activeJobsValue : undefined;
        const dispatch = dispatchValue && typeof dispatchValue === 'object'
            ? (dispatchValue as Record<string, unknown>)
            : undefined;
        const grpcConnected = typeof dispatch?.['grpcConnected'] === 'boolean'
            ? (dispatch['grpcConnected'] as boolean)
            : undefined;
        const lastConnectedAtUtc = typeof dispatch?.['lastConnectedAtUtc'] === 'string'
            ? (dispatch['lastConnectedAtUtc'] as string)
            : undefined;
        const lastFallbackAtUtc = typeof dispatch?.['lastFallbackAtUtc'] === 'string'
            ? (dispatch['lastFallbackAtUtc'] as string)
            : undefined;

        return {
            hostname,
            tags,
            capacity,
            activeJobs,
            dispatch: dispatch
                ? {
                    grpcConnected,
                    lastConnectedAtUtc,
                    lastFallbackAtUtc,
                }
                : undefined,
        };
    } catch {
        return {};
    }
};

const mapWorkerStatus = (worker: WorkerStatusModel): Worker => {
    const metadata = parseWorkerMetadata(worker.metadataJson);
    const capacity = metadata.capacity ?? 0;
    const activeJobs = metadata.activeJobs ?? 0;
    const loadPercent = capacity > 0 ? Math.min(100, (activeJobs / capacity) * 100) : 0;
    const loadLabel = capacity > 0 ? `${activeJobs}/${capacity}` : DEFAULT_LOAD_LABEL;
    const dispatch = metadata.dispatch;
    const dispatchState = dispatch?.grpcConnected
        ? 'Connected'
        : dispatch?.lastFallbackAtUtc
            ? 'Fallback'
            : 'Unknown';

    return {
        id: worker.instanceId,
        hostname: metadata.hostname ?? worker.instanceId,
        status: worker.isOnline ? 'Online' : 'Offline',
        dispatchState,
        dispatchLastConnectedAt: dispatch?.lastConnectedAtUtc,
        dispatchLastFallbackAt: dispatch?.lastFallbackAtUtc,
        lastHeartbeatAt: worker.lastSeenAtUtc ?? '',
        activeJobs,
        capacity,
        tags: metadata.tags ?? [],
        loadPercent,
        loadLabel,
    };
}

@Injectable()
export class WorkersStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    readonly workersResource = tenantRxResource<Worker[], { tenantId: string; environment: string }>({
        command: 'workers.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const { tenantId, environment } = params;
            if (!tenantId) return of([]);

            return this.api.listWorkers({ tenantId, environment }, requestOptions).pipe(
                map((response: WorkerListResponse) => (response.workers ?? []).map(mapWorkerStatus)),
                catchError(err => {
                    console.error('Failed to load workers', err);
                    return of<Worker[]>([]);
                })
            );
        }
    });

    readonly workers = computed(() => this.workersResource.value());
    readonly loading = computed(() => this.workersResource.isLoading());
    readonly error = computed(() => this.workersResource.error());

    // Metrics
    readonly activeWorkersCount = computed(() => this.workers().filter(r => r.status === 'Online').length);
    readonly totalCapacity = computed(() => this.workers().reduce((acc, r) => acc + (r.capacity || 0), 0));
    readonly busyThreads = computed(() => this.workers().reduce((acc, r) => acc + (r.activeJobs || 0), 0));

    refresh() {
        this.workersResource.reload();
    }
}
