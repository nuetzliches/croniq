import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { ApiClientResponse, IssueApiKeyRequest, IssueApiKeyResponse, UpsertApiClientRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient } from 'data-access';
import { Observable, catchError, map, of, tap } from 'rxjs';

@Injectable()
export class ApiAccessStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly clientsSignal = signal<ReadonlyArray<ApiClientResponse>>([]);
    private readonly clientsErrorSignal = signal<string | null>(null);

    private readonly clientsResource = tenantRxResource<ReadonlyArray<ApiClientResponse>, { tenantId: string }>({
        command: 'api-access.list',
        defaultValue: [],
        params: () => {
            return { tenantId: this.tenantContext.tenantId() ?? '' };
        },
        stream: ({ params, requestOptions }) => {
            this.clientsErrorSignal.set(null);

            const tenantId = params.tenantId.trim();
            if (!tenantId) {
                this.clientsErrorSignal.set('Required context is missing - unable to load API clients.');
                this.clientsSignal.set([]);
                return of([]);
            }

            return this.api.listTenantApiClients({ tenantId }, requestOptions).pipe(
                map((response) => Array.isArray(response) ? response as ApiClientResponse[] : []),
                tap((clients) => this.clientsSignal.set(clients)),
                catchError((error: unknown) => {
                    console.error('Failed to load API clients', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing API access permissions.',
                    });
                    if (authFailure) {
                        this.clientsErrorSignal.set(authFailure.message);
                        return of(this.clientsSignal());
                    }
                    this.clientsErrorSignal.set('Unable to load API clients from API.');
                    return of(this.clientsSignal());
                }),
            );
        },
    });

    readonly clients = this.clientsSignal.asReadonly();
    readonly isLoading = computed(() => this.clientsResource.isLoading());
    readonly error = this.clientsErrorSignal.asReadonly();

    upsertClient(request: UpsertApiClientRequest): Observable<UpsertApiClientRequest | null> {
        const tenant = this.tenantContext.tenantId();
        if (!tenant) return of(null);

        return this.api.upsertTenantApiClient(
            { tenantId: tenant },
            request
        ).pipe(
            tap(() => this.clientsResource.reload()),
            map(() => request)
        );
    }

    issueApiKey(request: IssueApiKeyRequest): Observable<IssueApiKeyResponse | null> {
        const tenant = this.tenantContext.tenantId();
        if (!tenant) return of(null);

        return this.api.issueTenantApiKey(
            { tenantId: tenant },
            request
        ).pipe(
            map(res => res as IssueApiKeyResponse)
        );
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
