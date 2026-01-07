import { Injectable, computed, inject, signal } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { of, catchError } from 'rxjs';

// TODO: Replace with generated type when available
export interface Runner {
    id: string;
    name?: string;
    hostname: string;
    status: 'Online' | 'Offline' | 'Draining';
    lastHeartbeatAt: string;
    activeJobs: number;
    capacity: number;
    tags: string[];
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
                // Map API response to Runner interface if needed
                // For now assuming 1:1 mapping or handled by client
                catchError(err => {
                    console.error('Failed to load runners', err);
                    return of([] as Runner[]);
                })
            ) as any; // Type assertion until schema is fully strict
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
