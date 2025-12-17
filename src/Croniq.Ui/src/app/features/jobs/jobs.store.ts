import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs } from '@core/time/clock';
import { CRONIQ_API_CLIENT, CallerContext, CroniqApiClient } from 'data-access';

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

export type JobRegistryEntry = {
    jobKey: string;
    description?: string;
};

export type ExecutionSummary = {
    executionId: string;
    jobKey?: string;
    status?: string;
    startedAt?: string;
};

@Injectable({ providedIn: 'root' })
export class JobsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly triggerLog = signal<ReadonlyArray<ManualTriggerEntry>>(seedManualTriggers());
    private readonly jobRegistrySignal = signal<ReadonlyArray<JobRegistryEntry>>([]);
    private readonly jobRegistryLoadingSignal = signal(false);
    private readonly jobRegistryErrorSignal = signal<string | null>(null);

    private readonly executionsSignal = signal<ReadonlyArray<ExecutionSummary>>([]);
    private readonly executionsLoadingSignal = signal(false);
    private readonly executionsErrorSignal = signal<string | null>(null);

    private readonly lastErrorSignal = signal<string | null>(null);

    readonly manualTriggers = this.triggerLog.asReadonly();
    readonly lastError = this.lastErrorSignal.asReadonly();
    readonly pendingCount = computed(() => this.manualTriggers().filter((entry) => entry.status === 'pending').length);

    readonly jobRegistry = this.jobRegistrySignal.asReadonly();
    readonly jobRegistryLoading = this.jobRegistryLoadingSignal.asReadonly();
    readonly jobRegistryError = this.jobRegistryErrorSignal.asReadonly();

    readonly executions = this.executionsSignal.asReadonly();
    readonly executionsLoading = this.executionsLoadingSignal.asReadonly();
    readonly executionsError = this.executionsErrorSignal.asReadonly();

    constructor() {
        queueMicrotask(() => {
            void this.refreshJobRegistry();
            void this.refreshExecutions();
        });
    }

    async refreshExecutions(params: { jobKey?: string; limit?: number } = {}): Promise<void> {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.executionsErrorSignal.set('TenantId is not set — select a tenant to load executions.');
            this.executionsSignal.set([]);
            return;
        }
        if (!environment.trim()) {
            this.executionsErrorSignal.set('Environment is not set — select an environment to load executions.');
            this.executionsSignal.set([]);
            return;
        }

        this.executionsLoadingSignal.set(true);
        this.executionsErrorSignal.set(null);
        try {
            const response = await this.api.listExecutions(
                {
                    tenantId,
                    environment,
                    jobKey: params.jobKey?.trim() || undefined,
                    limit: typeof params.limit === 'number' ? params.limit : 25,
                },
                this.tenantContext.createRequestOptions('executions.list', {
                    tenantId,
                    environment,
                }),
            );
            this.executionsSignal.set(normalizeExecutions(response));
        } catch (error) {
            console.error('Failed to load executions', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing executions permissions for this tenant.',
            });
            if (authFailure) {
                this.executionsErrorSignal.set(authFailure.message);
                this.executionsSignal.set([]);
                return;
            }
            this.executionsErrorSignal.set('Unable to load executions from API.');
            this.executionsSignal.set([]);
        } finally {
            this.executionsLoadingSignal.set(false);
        }
    }

    async refreshJobRegistry(): Promise<void> {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.jobRegistryErrorSignal.set('TenantId is not set — select a tenant to load jobs.');
            this.jobRegistrySignal.set([]);
            return;
        }
        if (!environment.trim()) {
            this.jobRegistryErrorSignal.set('Environment is not set — select an environment to load jobs.');
            this.jobRegistrySignal.set([]);
            return;
        }

        this.jobRegistryLoadingSignal.set(true);
        this.jobRegistryErrorSignal.set(null);
        try {
            const response = await this.api.listJobs(
                { tenantId, environment },
                this.tenantContext.createRequestOptions('jobs.list', {
                    tenantId,
                    environment,
                }),
            );
            this.jobRegistrySignal.set(normalizeJobRegistry(response));
        } catch (error) {
            console.error('Failed to load job registry', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing jobs permissions for this tenant.',
            });
            if (authFailure) {
                this.jobRegistryErrorSignal.set(authFailure.message);
                this.jobRegistrySignal.set([]);
                return;
            }
            this.jobRegistryErrorSignal.set('Unable to load jobs from API.');
        } finally {
            this.jobRegistryLoadingSignal.set(false);
        }
    }

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
            startedAt: nowIso(),
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
                completedAt: nowIso(),
            });
        } catch (error) {
            console.error('Failed to trigger job', error);
            this.lastErrorSignal.set('Unable to trigger job via API — entry retained locally.');
            this.updateEntry(entry.id, {
                status: 'error',
                completedAt: nowIso(),
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
    const now = nowMs();
    return [
        {
            id: createEntryId(),
            jobKey: 'nightly-billing-sweep',
            metadata: { tenant: 'cron-lab', source: 'ui-seed' },
            status: 'success',
            startedAt: isoFromEpochMs(now - 1000 * 60 * 45),
            completedAt: isoFromEpochMs(now - 1000 * 60 * 44),
        },
        {
            id: createEntryId(),
            jobKey: 'webhook-retry',
            metadata: { tenant: 'northwind', retries: '3' },
            status: 'error',
            startedAt: isoFromEpochMs(now - 1000 * 60 * 120),
            completedAt: isoFromEpochMs(now - 1000 * 60 * 119),
            error: 'Missing webhook scope',
        },
    ];
}

function createEntryId(): string {
    return typeof crypto !== 'undefined' && 'randomUUID' in crypto
        ? crypto.randomUUID()
    : `${nowMs()}-${Math.round(Math.random() * 1000)}`;
}

function normalizeJobRegistry(value: unknown): ReadonlyArray<JobRegistryEntry> {
    if (!Array.isArray(value)) {
        return [];
    }

    const entries: JobRegistryEntry[] = [];
    for (const item of value) {
        if (typeof item !== 'object' || item === null) {
            continue;
        }
        const record = item as Record<string, unknown>;
        const jobKey = typeof record['jobKey'] === 'string' ? record['jobKey'].trim() : '';
        if (!jobKey) {
            continue;
        }
        const description = typeof record['description'] === 'string' ? record['description'].trim() : undefined;
        entries.push({ jobKey, description: description || undefined });
    }

    return entries;
}

function normalizeExecutions(value: unknown): ReadonlyArray<ExecutionSummary> {
    if (!Array.isArray(value)) {
        return [];
    }

    const entries: ExecutionSummary[] = [];
    for (const item of value) {
        if (typeof item !== 'object' || item === null) {
            continue;
        }

        const record = item as Record<string, unknown>;
        const executionIdRaw =
            typeof record['executionId'] === 'string'
                ? record['executionId']
                : typeof record['id'] === 'string'
                    ? record['id']
                    : '';
        const executionId = executionIdRaw.trim();
        if (!executionId) {
            continue;
        }

        const jobKey = typeof record['jobKey'] === 'string' ? record['jobKey'].trim() : undefined;
        const statusRaw =
            typeof record['status'] === 'string'
                ? record['status']
                : typeof record['status'] === 'number'
                    ? String(record['status'])
                    : undefined;
        const status = statusRaw?.trim() || undefined;

        const startedAtRaw =
            typeof record['startedAtUtc'] === 'string'
                ? record['startedAtUtc']
                : typeof record['startedAt'] === 'string'
                    ? record['startedAt']
                    : undefined;
        const startedAt = startedAtRaw?.trim() || undefined;

        entries.push({ executionId, jobKey: jobKey || undefined, status, startedAt });
    }

    return entries;
}
