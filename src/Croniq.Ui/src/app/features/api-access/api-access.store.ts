import { Injectable, computed, inject } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { ApiClientResponse, UpsertApiClientRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { map, of, tap } from 'rxjs';

@Injectable()
export class ApiAccessStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    readonly clientsResource = tenantRxResource<ApiClientResponse[], { tenantId: string }>({
        command: 'listApiClients',
        defaultValue: [],
        params: () => {
            const tenant = this.tenantContext.tenantId();
            if (!tenant) return { tenantId: '' }; // Should ideally handle no-tenant case by not fetching
            return { tenantId: tenant };
        },
        stream: (args) => {
            if (!args.params.tenantId) return of([]);
            return this.api.listTenantApiClients(args.params).pipe(
                map(res => res as ApiClientResponse[])
            );
        },
    });

    readonly clients = computed(() => this.clientsResource.value() ?? []);
    readonly isLoading = this.clientsResource.isLoading;
    readonly error = this.clientsResource.error;

    upsertClient(request: UpsertApiClientRequest) {
        const tenant = this.tenantContext.tenantId();
        if (!tenant) return;

        this.api.upsertTenantApiClient(
            { tenantId: tenant },
            request
        )
            .pipe(
                tap(() => this.clientsResource.reload())
            )
            .subscribe();
    }

    deleteClient(clientId: string) {
        const tenant = this.tenantContext.tenantId();
        if (!tenant) return;

        this.api.deleteTenantApiClient({
            tenantId: tenant,
            clientId
        })
            .pipe(
                tap(() => this.clientsResource.reload())
            )
            .subscribe();
    }
}
