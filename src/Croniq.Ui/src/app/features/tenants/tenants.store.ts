import { Injectable, computed, inject, signal } from '@angular/core';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { nowIso, nowMs } from '@core/time/clock';
import { IssueApiKeyRequest, IssueTokenRequest } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CroniqApiClient, TenantApiClientParams, TenantApiKeyParams, TenantScopedParams } from 'data-access';
import { EMPTY, catchError, finalize, map, of, tap, type Observable } from 'rxjs';

export type ApiKeyActionType = 'issue' | 'issue-token' | 'rotate' | 'delete';
export type ApiKeyActionStatus = 'pending' | 'success' | 'error';
export type ApiKeyActivityEntry = {
    id: string;
    tenantId: string;
    environment?: string | null;
    action: ApiKeyActionType;
    status: ApiKeyActionStatus;
    detail?: string;
    recordedAt: string;
};

export type ApiClientSnapshot = {
    tenantId: string;
    clientId: string;
    environment?: string | null;
    payload: unknown;
    fetchedAt: string;
};

type ApiClientLookupParams = TenantApiClientParams & { trigger: number };

@Injectable()
export class TenantsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly activityLog = signal<ReadonlyArray<ApiKeyActivityEntry>>([]);
    private readonly busySignal = signal(false);
    private readonly lastErrorSignal = signal<string | null>(null);

    private readonly apiClientLookupParamsSignal = signal<ApiClientLookupParams>({
        trigger: 0,
        tenantId: '',
        clientId: '',
        environment: null,
    });

    private readonly apiClientLookupResource = tenantRxResource<ApiClientSnapshot | null, ApiClientLookupParams>({
        command: 'tenants.lookup-api-client',
        defaultValue: null,
        params: () => this.apiClientLookupParamsSignal(),
        callerContextOverrides: (params) => ({
            tenantId: params.tenantId,
            environment: params.environment ?? undefined,
        }),
        stream: ({ params, requestOptions }) => {
            if (params.trigger === 0) {
                return of(null);
            }

            const request$ = this.api.getTenantApiClient(params, requestOptions);

            return request$.pipe(
                map((payload) => ({
                    tenantId: params.tenantId,
                    clientId: params.clientId,
                    environment: params.environment ?? null,
                    payload,
                    fetchedAt: nowIso(),
                })),
                tap(() => this.lastErrorSignal.set(null)),
                catchError((error) => {
                    console.error('Unable to fetch API client', error);
                    this.lastErrorSignal.set('API client lookup failed.');
                    return of(null);
                }),
            );
        },
    });

    readonly activity = this.activityLog.asReadonly();
    readonly lastLookup = computed(() => this.apiClientLookupResource.value());
    readonly busy = computed(() => this.busySignal() || this.apiClientLookupResource.isLoading());
    readonly lastError = this.lastErrorSignal.asReadonly();

    issueApiKey(params: TenantScopedParams, payload: IssueApiKeyRequest): void {
        const tenantId = (params.tenantId ?? '').trim();
        if (!tenantId) {
            this.lastErrorSignal.set('Missing tenantId. Sign in again and retry.');
            return;
        }

        const resolvedEnvironment = this.resolveEnvironmentTag(payload.environmentTag);
        const resolvedPayload: IssueApiKeyRequest = {
            ...payload,
            environmentTag: resolvedEnvironment,
        };

        const entry = this.appendActivity(tenantId, resolvedEnvironment ?? null, 'issue');
        const requestOptions = this.tenantContext.createRequestOptions('tenants.issue-api-key', {
            tenantId,
            environment: resolvedEnvironment ?? undefined,
        });

        this.runWithBusy(
            this.api.issueTenantApiKey({ tenantId }, resolvedPayload, requestOptions).pipe(
                tap((response) => {
                    this.patchActivity(entry.id, {
                        status: 'success',
                        detail: summarizeResponse(response),
                    });
                    this.lastErrorSignal.set(null);
                }),
                catchError((error: unknown) => {
                    console.error('Unable to issue API key', error);
                    this.patchActivity(entry.id, {
                        status: 'error',
                        detail: error instanceof Error ? error.message : 'Unknown error',
                    });
                    this.lastErrorSignal.set('API key issuance failed — check activity feed for details.');
                    return EMPTY;
                }),
            ),
        );
    }

    issueApiClientToken(params: TenantApiClientParams, payload: IssueTokenRequest): void {
        const tenantId = (params.tenantId ?? '').trim();
        if (!tenantId) {
            this.lastErrorSignal.set('Missing tenantId. Sign in again and retry.');
            return;
        }

        const resolvedEnvironment = this.resolveEnvironmentTag(params.environment ?? null);
        const entry = this.appendActivity(tenantId, resolvedEnvironment ?? null, 'issue-token');
        const requestOptions = this.tenantContext.createRequestOptions('tenants.issue-api-client-token', {
            tenantId,
            environment: resolvedEnvironment ?? undefined,
        });

        this.runWithBusy(
            this.api.issueApiClientToken({ ...params, tenantId, environment: resolvedEnvironment ?? undefined }, payload, requestOptions).pipe(
                tap((response) => {
                    this.patchActivity(entry.id, {
                        status: 'success',
                        detail: summarizeResponse(response),
                    });
                    this.lastErrorSignal.set(null);
                }),
                catchError((error: unknown) => {
                    console.error('Unable to issue API client token', error);
                    this.patchActivity(entry.id, {
                        status: 'error',
                        detail: error instanceof Error ? error.message : 'Unknown error',
                    });
                    this.lastErrorSignal.set('Token issuance failed — operator should review logs.');
                    return EMPTY;
                }),
            ),
        );
    }

    rotateApiKey(params: TenantApiKeyParams): void {
        const tenantId = (params.tenantId ?? '').trim();
        if (!tenantId) {
            this.lastErrorSignal.set('Missing tenantId. Sign in again and retry.');
            return;
        }

        const resolvedEnvironment = this.resolveEnvironmentTag(params.environment ?? null);
        const entry = this.appendActivity(tenantId, resolvedEnvironment ?? null, 'rotate');
        this.runWithBusy(
            this.api
                .rotateTenantApiKey(
                    { ...params, tenantId, environment: resolvedEnvironment ?? undefined },
                    this.tenantContext.createRequestOptions('tenants.rotate-api-key', {
                        tenantId,
                        environment: resolvedEnvironment ?? undefined,
                    }),
                )
                .pipe(
                    tap(() => {
                        this.patchActivity(entry.id, { status: 'success', detail: 'Rotation scheduled' });
                        this.lastErrorSignal.set(null);
                    }),
                    catchError((error: unknown) => {
                        console.error('Unable to rotate API key', error);
                        this.patchActivity(entry.id, {
                            status: 'error',
                            detail: error instanceof Error ? error.message : 'Unknown error',
                        });
                        this.lastErrorSignal.set('Rotation failed — operator should review logs.');
                        return EMPTY;
                    }),
                ),
        );
    }

    deleteApiKey(params: TenantApiKeyParams): void {
        const tenantId = (params.tenantId ?? '').trim();
        if (!tenantId) {
            this.lastErrorSignal.set('Missing tenantId. Sign in again and retry.');
            return;
        }

        const resolvedEnvironment = this.resolveEnvironmentTag(params.environment ?? null);
        const entry = this.appendActivity(tenantId, resolvedEnvironment ?? null, 'delete');
        this.runWithBusy(
            this.api
                .deleteTenantApiKey(
                    { ...params, tenantId, environment: resolvedEnvironment ?? undefined },
                    this.tenantContext.createRequestOptions('tenants.delete-api-key', {
                        tenantId,
                        environment: resolvedEnvironment ?? undefined,
                    }),
                )
                .pipe(
                    tap(() => {
                        this.patchActivity(entry.id, { status: 'success', detail: 'Key deleted' });
                        this.lastErrorSignal.set(null);
                    }),
                    catchError((error: unknown) => {
                        console.error('Unable to delete API key', error);
                        this.patchActivity(entry.id, {
                            status: 'error',
                            detail: error instanceof Error ? error.message : 'Unknown error',
                        });
                        this.lastErrorSignal.set('Deletion failed — key may already be inactive.');
                        return EMPTY;
                    }),
                ),
        );
    }

    lookupApiClient(params: TenantApiClientParams): void {
        const tenantId = (params.tenantId ?? '').trim();
        if (!tenantId) {
            this.lastErrorSignal.set('Missing tenantId. Sign in again and retry.');
            return;
        }

        const resolvedEnvironment = this.resolveEnvironmentTag(params.environment ?? null);

        this.apiClientLookupParamsSignal.set({
            trigger: nowMs(),
            tenantId,
            clientId: params.clientId,
            environment: resolvedEnvironment ?? null,
        });
        this.apiClientLookupResource.reload();
    }

    private resolveEnvironmentTag(explicitEnvironment: string | null | undefined): string | null {
        const fromParams = explicitEnvironment?.trim() ?? '';
        if (fromParams) {
            return fromParams;
        }
        const fromContext = this.tenantContext.snapshot().environment?.trim() ?? '';
        return fromContext || null;
    }

    private appendActivity(
        tenantId: string,
        environment: string | null,
        action: ApiKeyActionType,
    ): ApiKeyActivityEntry {
        const entry: ApiKeyActivityEntry = {
            id: createEntryId(),
            tenantId,
            environment,
            action,
            status: 'pending',
            recordedAt: nowIso(),
        };
        this.activityLog.set([entry, ...this.activityLog()]);
        return entry;
    }

    private patchActivity(id: string, patch: Partial<ApiKeyActivityEntry>): void {
        this.activityLog.set(this.activityLog().map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)));
    }

    private runWithBusy(operation$: Observable<unknown>): void {
        if (this.busySignal()) {
            return;
        }
        this.busySignal.set(true);

        operation$
            .pipe(
                finalize(() => {
                    this.busySignal.set(false);
                }),
            )
            .subscribe();
    }

}

function createEntryId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${nowMs()}-${Math.round(Math.random() * 1000)}`;
}

function summarizeResponse(value: unknown): string {
    if (value == null) {
        return 'No payload returned';
    }
    if (typeof value === 'string') {
        return value.slice(0, 120);
    }
    try {
        return JSON.stringify(value).slice(0, 160);
    } catch {
        return 'Response ready but not serializable';
    }
}
