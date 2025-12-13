import { Injectable, inject, signal } from '@angular/core';

import { IssueApiKeyRequest } from '@croniq/api-schema';
import {
    CRONIQ_API_CLIENT,
    CroniqApiClient,
    TenantApiClientParams,
    TenantApiKeyParams,
    TenantScopedParams,
} from 'data-access';

export type ApiKeyActionType = 'issue' | 'rotate' | 'delete';
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

@Injectable({ providedIn: 'root' })
export class TenantsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);

    private readonly activityLog = signal<ReadonlyArray<ApiKeyActivityEntry>>(seedActivity());
    private readonly lastLookupSignal = signal<ApiClientSnapshot | null>(null);
    private readonly busySignal = signal(false);
    private readonly lastErrorSignal = signal<string | null>(null);

    readonly activity = this.activityLog.asReadonly();
    readonly lastLookup = this.lastLookupSignal.asReadonly();
    readonly busy = this.busySignal.asReadonly();
    readonly lastError = this.lastErrorSignal.asReadonly();

    async issueApiKey(params: TenantScopedParams, payload: IssueApiKeyRequest): Promise<void> {
        const entry = this.appendActivity(params.tenantId, payload.environmentTag ?? null, 'issue');
        await this.runWithBusy(async () => {
            try {
                const response = await this.api.issueTenantApiKey(params, payload);
                this.patchActivity(entry.id, {
                    status: 'success',
                    detail: summarizeResponse(response),
                });
                this.lastErrorSignal.set(null);
            } catch (error) {
                console.error('Unable to issue API key', error);
                this.patchActivity(entry.id, {
                    status: 'error',
                    detail: error instanceof Error ? error.message : 'Unknown error',
                });
                this.lastErrorSignal.set('API key issuance failed — check activity feed for details.');
            }
        });
    }

    async rotateApiKey(params: TenantApiKeyParams): Promise<void> {
        const entry = this.appendActivity(params.tenantId, params.environment ?? null, 'rotate');
        await this.runWithBusy(async () => {
            try {
                await this.api.rotateTenantApiKey(params);
                this.patchActivity(entry.id, { status: 'success', detail: 'Rotation scheduled' });
                this.lastErrorSignal.set(null);
            } catch (error) {
                console.error('Unable to rotate API key', error);
                this.patchActivity(entry.id, {
                    status: 'error',
                    detail: error instanceof Error ? error.message : 'Unknown error',
                });
                this.lastErrorSignal.set('Rotation failed — operator should review logs.');
            }
        });
    }

    async deleteApiKey(params: TenantApiKeyParams): Promise<void> {
        const entry = this.appendActivity(params.tenantId, params.environment ?? null, 'delete');
        await this.runWithBusy(async () => {
            try {
                await this.api.deleteTenantApiKey(params);
                this.patchActivity(entry.id, { status: 'success', detail: 'Key deleted' });
                this.lastErrorSignal.set(null);
            } catch (error) {
                console.error('Unable to delete API key', error);
                this.patchActivity(entry.id, {
                    status: 'error',
                    detail: error instanceof Error ? error.message : 'Unknown error',
                });
                this.lastErrorSignal.set('Deletion failed — key may already be inactive.');
            }
        });
    }

    async lookupApiClient(params: TenantApiClientParams): Promise<void> {
        await this.runWithBusy(async () => {
            try {
                const payload = await this.api.getTenantApiClient(params);
                this.lastLookupSignal.set({
                    tenantId: params.tenantId,
                    clientId: params.clientId,
                    environment: params.environment ?? null,
                    payload,
                    fetchedAt: new Date().toISOString(),
                });
                this.lastErrorSignal.set(null);
            } catch (error) {
                console.error('Unable to fetch API client', error);
                this.lastErrorSignal.set('API client lookup failed.');
            }
        });
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
            recordedAt: new Date().toISOString(),
        };
        this.activityLog.set([entry, ...this.activityLog()]);
        return entry;
    }

    private patchActivity(id: string, patch: Partial<ApiKeyActivityEntry>): void {
        this.activityLog.set(this.activityLog().map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)));
    }

    private async runWithBusy(operation: () => Promise<void>): Promise<void> {
        if (this.busySignal()) {
            return;
        }
        this.busySignal.set(true);
        try {
            await operation();
        } finally {
            this.busySignal.set(false);
        }
    }
}

function seedActivity(): ReadonlyArray<ApiKeyActivityEntry> {
    return [
        {
            id: createEntryId(),
            tenantId: 'cron-lab',
            environment: 'production',
            action: 'issue',
            status: 'success',
            detail: 'Key issued via CLI',
            recordedAt: new Date(Date.now() - 1000 * 60 * 90).toISOString(),
        },
        {
            id: createEntryId(),
            tenantId: 'northwind',
            environment: 'staging',
            action: 'rotate',
            status: 'error',
            detail: 'Scope mismatch',
            recordedAt: new Date(Date.now() - 1000 * 60 * 200).toISOString(),
        },
    ];
}

function createEntryId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.round(Math.random() * 1000)}`;
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
