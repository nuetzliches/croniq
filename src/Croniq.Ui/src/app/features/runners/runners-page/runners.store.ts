import { Injectable, computed, inject } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { RunnerListResponse, RunnerStatusModel } from '@croniq/api-schema';
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
    loadPercent: number;
    loadLabel: string;
}

interface RunnerMetadata {
    hostname?: string;
    tags?: string[];
    capacity?: number;
    activeJobs?: number;
}

const DEFAULT_LOAD_LABEL = 'n/a';

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
        const hostname = typeof record.hostname === 'string' ? record.hostname : undefined;
        const tags = Array.isArray(record.tags) ? record.tags.filter((tag): tag is string => typeof tag === 'string') : undefined;
        const capacity = typeof record.capacity === 'number' ? record.capacity : undefined;
        const activeJobs = typeof record.activeJobs === 'number' ? record.activeJobs : undefined;

        return {
            hostname,
            tags,
            capacity,
            activeJobs,
        };
    } catch {
        return {};
    }
};

const mapRunnerStatus = (runner: RunnerStatusModel): Runner => {
    const metadata = parseRunnerMetadata(runner.metadataJson);
    const capacity = metadata.capacity ?? 0;
    const activeJobs = metadata.activeJobs ?? 0;
    const loadPercent = capacity > 0 ? Math.min(100, (activeJobs / capacity) * 100) : 0;
    const loadLabel = capacity > 0 ? `${activeJobs}/${capacity}` : DEFAULT_LOAD_LABEL;

    return {
        id: runner.runnerId,
        hostname: metadata.hostname ?? runner.runnerId,
        status: runner.isOnline ? 'Online' : 'Offline',
        lastHeartbeatAt: runner.lastSeenAtUtc,
        activeJobs,
        capacity,
        tags: metadata.tags ?? [],
        loadPercent,
        loadLabel,
    };
}

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

            return this.api.listRunners({ tenantId, environment }, requestOptions).pipe(
                map((response: RunnerListResponse) => (response.runners ?? []).map(mapRunnerStatus)),
                catchError(err => {
                    console.error('Failed to load runners', err);
                    return of<Runner[]>([]);
                })
            );
        }
    });

    readonly runners = computed(() => this.runnersResource.value());
    readonly loading = computed(() => this.runnersResource.isLoading());
    readonly error = computed(() => this.runnersResource.error());

    // Metrics
    readonly activeRunnersCount = computed(() => this.runners().filter(r => r.status === 'Online').length);
    readonly totalCapacity = computed(() => this.runners().reduce((acc, r) => acc + (r.capacity || 0), 0));
    readonly busyThreads = computed(() => this.runners().reduce((acc, r) => acc + (r.activeJobs || 0), 0));

    refresh() {
        this.runnersResource.reload();
    }
}
