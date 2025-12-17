import { HttpErrorResponse } from '@angular/common/http';
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

export type JobDetail = {
    jobId: string;
    jobKey?: string;
    description?: string;
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

    private readonly jobDetailSignal = signal<JobDetail | null>(null);
    private readonly jobDetailLoadingSignal = signal(false);
    private readonly jobDetailErrorSignal = signal<string | null>(null);
    private readonly deleteJobLoadingSignal = signal(false);
    private readonly deleteJobErrorSignal = signal<string | null>(null);

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

    readonly jobDetail = this.jobDetailSignal.asReadonly();
    readonly jobDetailLoading = this.jobDetailLoadingSignal.asReadonly();
    readonly jobDetailError = this.jobDetailErrorSignal.asReadonly();
    readonly deleteJobLoading = this.deleteJobLoadingSignal.asReadonly();
    readonly deleteJobError = this.deleteJobErrorSignal.asReadonly();

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

    async refreshJobDetail(jobId: string): Promise<void> {
        const trimmedId = jobId.trim();
        if (!trimmedId) {
            this.jobDetailErrorSignal.set('Job id is required to load job detail.');
            this.jobDetailSignal.set(null);
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.jobDetailErrorSignal.set('TenantId is not set — select a tenant to load job detail.');
            this.jobDetailSignal.set(null);
            return;
        }
        if (!environment.trim()) {
            this.jobDetailErrorSignal.set('Environment is not set — select an environment to load job detail.');
            this.jobDetailSignal.set(null);
            return;
        }

        this.jobDetailLoadingSignal.set(true);
        this.jobDetailErrorSignal.set(null);
        try {
            const response = await this.api.getJob(
                { tenantId, environment, jobId: trimmedId },
                this.tenantContext.createRequestOptions('jobs.get', {
                    tenantId,
                    environment,
                }),
            );
            this.jobDetailSignal.set(normalizeJobDetail(response, trimmedId));
        } catch (error) {
            console.error('Failed to load job detail', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing jobs permissions for this tenant.',
            });
            if (authFailure) {
                this.jobDetailErrorSignal.set(authFailure.message);
                this.jobDetailSignal.set(null);
                return;
            }
            if (error instanceof HttpErrorResponse && error.status === 404) {
                this.jobDetailErrorSignal.set('Job not found (404) — verify the job id in the registry.');
                this.jobDetailSignal.set(null);
                return;
            }
            this.jobDetailErrorSignal.set('Unable to load job detail from API.');
            this.jobDetailSignal.set(null);
        } finally {
            this.jobDetailLoadingSignal.set(false);
        }
    }

    async deleteJob(jobId: string): Promise<void> {
        const trimmedId = jobId.trim();
        if (!trimmedId) {
            this.deleteJobErrorSignal.set('Job id is required before deleting.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.deleteJobErrorSignal.set('TenantId is not set — select a tenant to delete jobs.');
            return;
        }
        if (!environment.trim()) {
            this.deleteJobErrorSignal.set('Environment is not set — select an environment to delete jobs.');
            return;
        }

        this.deleteJobLoadingSignal.set(true);
        this.deleteJobErrorSignal.set(null);
        try {
            await this.api.deleteJob(
                { tenantId, environment, jobId: trimmedId },
                this.tenantContext.createRequestOptions('jobs.delete', {
                    tenantId,
                    environment,
                }),
            );

            const current = this.jobDetailSignal();
            if (current?.jobId === trimmedId || current?.jobKey === trimmedId) {
                this.jobDetailSignal.set(null);
            }
            this.jobRegistrySignal.set(this.jobRegistrySignal().filter((entry) => entry.jobKey !== trimmedId));
            void this.refreshJobRegistry();
        } catch (error) {
            console.error('Failed to delete job', error);
            const authFailure = authFailureFromError(error, {
                forbidden: 'Forbidden (403) — your token is missing jobs permissions for this tenant.',
            });
            if (authFailure) {
                this.deleteJobErrorSignal.set(authFailure.message);
                return;
            }
            if (error instanceof HttpErrorResponse && error.status === 404) {
                this.deleteJobErrorSignal.set('Job not found (404) — it may have already been deleted.');
                return;
            }
            this.deleteJobErrorSignal.set('Unable to delete job via API.');
        } finally {
            this.deleteJobLoadingSignal.set(false);
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

function normalizeJobDetail(value: unknown, fallbackId: string): JobDetail | null {
    if (typeof value !== 'object' || value === null) {
        return { jobId: fallbackId };
    }

    const record = value as Record<string, unknown>;
    const jobIdRaw =
        typeof record['jobId'] === 'string'
            ? record['jobId']
            : typeof record['id'] === 'string'
                ? record['id']
                : typeof record['jobKey'] === 'string'
                    ? record['jobKey']
                    : fallbackId;
    const jobId = jobIdRaw.trim() || fallbackId;

    const jobKey = typeof record['jobKey'] === 'string' ? record['jobKey'].trim() : undefined;
    const description = typeof record['description'] === 'string' ? record['description'].trim() : undefined;
    return {
        jobId,
        jobKey: jobKey || undefined,
        description: description || undefined,
    };
}
