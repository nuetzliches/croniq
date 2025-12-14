import { Injectable, computed, inject, signal } from '@angular/core';

import { CRONIQ_API_CLIENT, CallerContext, CroniqApiClient } from 'data-access';

import { TenantContextService } from '../../core/tenant-context/tenant-context.service';

export type ManualTriggerStatus = 'pending' | 'success' | 'error';
export type ManualTriggerEntry = {
    id: string;
    jobKey: string;
    metadata: Record<string, string>;
    status: ManualTriggerStatus;
    startedAt: string;
    completedAt?: string;
    error?: string;
};

@Injectable({ providedIn: 'root' })
export class JobsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly triggerLog = signal<ReadonlyArray<ManualTriggerEntry>>(seedManualTriggers());
    private readonly lastErrorSignal = signal<string | null>(null);

    readonly manualTriggers = this.triggerLog.asReadonly();
    readonly lastError = this.lastErrorSignal.asReadonly();
    readonly pendingCount = computed(() => this.manualTriggers().filter((entry) => entry.status === 'pending').length);

    async triggerJob(jobKey: string, metadata: Record<string, string>): Promise<void> {
        const trimmedKey = jobKey.trim();
        if (!trimmedKey) {
            this.lastErrorSignal.set('Job key is required before triggering.');
            return;
        }

        const entry: ManualTriggerEntry = {
            id: createEntryId(),
            jobKey: trimmedKey,
            metadata,
            status: 'pending',
            startedAt: new Date().toISOString(),
        };

        this.triggerLog.set([entry, ...this.triggerLog()]);
        this.lastErrorSignal.set(null);

        try {
            await this.api.triggerJob(
                { jobKey: trimmedKey, metadata },
                this.tenantContext.createRequestOptions(
                    `jobs.trigger:${trimmedKey}`,
                    this.buildCallerOverrides(metadata)
                ),
            );
            this.updateEntry(entry.id, {
                status: 'success',
                completedAt: new Date().toISOString(),
            });
        } catch (error) {
            console.error('Failed to trigger job', error);
            this.lastErrorSignal.set('Unable to trigger job via API — entry retained locally.');
            this.updateEntry(entry.id, {
                status: 'error',
                completedAt: new Date().toISOString(),
                error: error instanceof Error ? error.message : 'Unknown error',
            });
        }
    }

    private updateEntry(id: string, patch: Partial<ManualTriggerEntry>): void {
        this.triggerLog.set(
            this.triggerLog().map((entry) => (entry.id === id ? { ...entry, ...patch } : entry))
        );
    }

    private buildCallerOverrides(metadata: Record<string, string>): Partial<CallerContext> {
        const tenantId = metadata['tenant']?.trim();
        const environment = metadata['environment']?.trim() ?? metadata['env']?.trim();
        const actor = metadata['actor']?.trim();
        const source = metadata['source']?.trim();
        return {
            tenantId: tenantId || undefined,
            environment: environment || undefined,
            actor: actor || undefined,
            source: source || undefined,
        };
    }
}

function seedManualTriggers(): ReadonlyArray<ManualTriggerEntry> {
    return [
        {
            id: createEntryId(),
            jobKey: 'nightly-billing-sweep',
            metadata: { tenant: 'cron-lab', source: 'ui-seed' },
            status: 'success',
            startedAt: new Date(Date.now() - 1000 * 60 * 45).toISOString(),
            completedAt: new Date(Date.now() - 1000 * 60 * 44).toISOString(),
        },
        {
            id: createEntryId(),
            jobKey: 'webhook-retry',
            metadata: { tenant: 'northwind', retries: '3' },
            status: 'error',
            startedAt: new Date(Date.now() - 1000 * 60 * 120).toISOString(),
            completedAt: new Date(Date.now() - 1000 * 60 * 119).toISOString(),
            error: 'Missing webhook scope',
        },
    ];
}

function createEntryId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.round(Math.random() * 1000)}`;
}
