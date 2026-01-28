import { Injectable, computed, inject } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { ExecutionResponse } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { map, of } from 'rxjs';

@Injectable()
export class ExecutionsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    readonly executionsResource = tenantRxResource<ExecutionResponse[], { tenantId: string; environment?: string | null }>({
        command: 'listExecutions',
        defaultValue: [],
        params: () => {
            const tenant = this.tenantContext.tenantId();
            const env = this.tenantContext.environment();
            if (!tenant) return { tenantId: '' };
            return { tenantId: tenant, environment: env };
        },
        stream: (args) => {
            if (!args.params.tenantId) return of([]);

            // We cast internal unknown to ExecutionResponse[] for now until client is strictly typed
            return this.api.listExecutions({
                tenantId: args.params.tenantId,
                environment: args.params.environment,
                limit: 50
            }).pipe(
                map(res => res as ExecutionResponse[])
            );
        },
    });

    readonly executions = computed(() => this.executionsResource.value() ?? []);
    readonly isLoading = this.executionsResource.isLoading;
    readonly loading = computed(() => this.executionsResource.isLoading());
    readonly error = computed(() => this.executionsResource.error());

    refresh() {
        this.executionsResource.reload();
    }

    fetchLogs(executionId: string) {
        const tenant = this.tenantContext.tenantId();
        if (!tenant) return of('');

        return this.api.fetchExecutionLogs({
            tenantId: tenant,
            executionId
        });
    }
}
