import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs } from '@core/time/clock';
import { CRONIQ_API_CLIENT, CallerContext, CroniqApiClient } from 'data-access';
import { EMPTY, catchError, finalize, map, of, tap } from 'rxjs';

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

@Injectable()
export class JobsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly triggerLog = signal<ReadonlyArray<ManualTriggerEntry>>(seedManualTriggers());
    private readonly jobRegistrySignal = signal<ReadonlyArray<JobRegistryEntry>>([]);
    private readonly jobRegistryErrorSignal = signal<string | null>(null);

    private readonly jobRegistryResource = tenantRxResource<ReadonlyArray<JobRegistryEntry>, { tenantId: string; environment: string }>({
        command: 'jobs.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            this.jobRegistryErrorSignal.set(null);

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();

            if (!tenantId) {
                this.jobRegistryErrorSignal.set('Required context is missing — unable to load jobs.');
                this.jobRegistrySignal.set([]);
                return of([]);
            }
            if (!environment) {
                this.jobRegistryErrorSignal.set('Environment is not set — select an environment to load jobs.');
                this.jobRegistrySignal.set([]);
                return of([]);
            }

            const request$ = this.api.listJobs({ tenantId, environment }, requestOptions);

            return request$.pipe(
                map((response) => normalizeJobRegistry(response)),
                tap((normalized) => {
                    this.jobRegistrySignal.set(normalized);
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load job registry', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.jobRegistryErrorSignal.set(authFailure.message);
                        this.jobRegistrySignal.set([]);
                        return of([]);
                    }

                    this.jobRegistryErrorSignal.set('Unable to load jobs from API.');
                    this.jobRegistrySignal.set([]);
                    return of([]);
                }),
            );
        },
    });

    private readonly executionsSignal = signal<ReadonlyArray<ExecutionSummary>>([]);
    private readonly executionsErrorSignal = signal<string | null>(null);

    private readonly executionsQuery = signal<{ jobKey?: string; limit: number }>({
        jobKey: undefined,
        limit: 25,
    });

    private readonly executionsResource = tenantRxResource<ReadonlyArray<ExecutionSummary>, { tenantId: string; environment: string; jobKey?: string; limit: number }>({
        command: 'executions.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            const query = this.executionsQuery();
            return {
                tenantId,
                environment,
                jobKey: query.jobKey?.trim() || undefined,
                limit: query.limit,
            };
        },
        stream: ({ params, requestOptions }) => {
            this.executionsErrorSignal.set(null);

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();

            if (!tenantId) {
                this.executionsErrorSignal.set('Required context is missing — unable to load executions.');
                this.executionsSignal.set([]);
                return of([]);
            }
            if (!environment) {
                this.executionsErrorSignal.set('Environment is not set — select an environment to load executions.');
                this.executionsSignal.set([]);
                return of([]);
            }

            const request$ = this.api.listExecutions(
                {
                    tenantId,
                    environment,
                    jobKey: params.jobKey,
                    limit: typeof params.limit === 'number' ? params.limit : 25,
                },
                requestOptions,
            );

            return request$.pipe(
                map((response) => normalizeExecutions(response)),
                tap((normalized) => {
                    this.executionsSignal.set(normalized);
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load executions', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden:
                            'Forbidden (403) — your token is missing executions permissions.',
                    });
                    if (authFailure) {
                        this.executionsErrorSignal.set(authFailure.message);
                        this.executionsSignal.set([]);
                        return of([]);
                    }
                    this.executionsErrorSignal.set('Unable to load executions from API.');
                    this.executionsSignal.set([]);
                    return of([]);
                }),
            );
        },
    });

    private readonly jobDetailSignal = signal<JobDetail | null>(null);
    private readonly jobDetailJobIdSignal = signal<string | null>(null);
    private readonly jobDetailErrorSignal = signal<string | null>(null);
    private readonly jobDetailResource = tenantRxResource<JobDetail | null, { tenantId: string; environment: string; jobId: string | null }>({
        command: 'jobs.get',
        defaultValue: null,
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return {
                tenantId,
                environment,
                jobId: this.jobDetailJobIdSignal(),
            };
        },
        stream: ({ params, requestOptions }) => {
            this.jobDetailErrorSignal.set(null);

            const trimmedId = params.jobId?.trim() ?? '';
            if (!trimmedId) {
                this.jobDetailSignal.set(null);
                return of(null);
            }

            const tenantId = params.tenantId.trim();
            const environment = params.environment.trim();
            if (!tenantId) {
                this.jobDetailErrorSignal.set('Required context is missing — unable to load job detail.');
                this.jobDetailSignal.set(null);
                return of(null);
            }
            if (!environment) {
                this.jobDetailErrorSignal.set('Environment is not set — select an environment to load job detail.');
                this.jobDetailSignal.set(null);
                return of(null);
            }

            const request$ = this.api.getJob({ tenantId, environment, jobId: trimmedId }, requestOptions);

            return request$.pipe(
                map((response) => normalizeJobDetail(response, trimmedId)),
                tap((normalized) => {
                    this.jobDetailSignal.set(normalized);
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load job detail', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.jobDetailErrorSignal.set(authFailure.message);
                        this.jobDetailSignal.set(null);
                        return of(null);
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.jobDetailErrorSignal.set('Job not found (404) — verify the job id in the registry.');
                        this.jobDetailSignal.set(null);
                        return of(null);
                    }
                    this.jobDetailErrorSignal.set('Unable to load job detail from API.');
                    this.jobDetailSignal.set(null);
                    return of(null);
                }),
            );
        },
    });

    private readonly deleteJobLoadingSignal = signal(false);
    private readonly deleteJobErrorSignal = signal<string | null>(null);

    private readonly lastErrorSignal = signal<string | null>(null);

    readonly manualTriggers = this.triggerLog.asReadonly();
    readonly lastError = this.lastErrorSignal.asReadonly();
    readonly pendingCount = computed(() => this.manualTriggers().filter((entry) => entry.status === 'pending').length);

    readonly jobRegistry = this.jobRegistrySignal.asReadonly();
    readonly jobRegistryLoading = computed(() => this.jobRegistryResource.isLoading());
    readonly jobRegistryError = this.jobRegistryErrorSignal.asReadonly();

    readonly executions = this.executionsSignal.asReadonly();
    readonly executionsLoading = computed(() => this.executionsResource.isLoading());
    readonly executionsError = this.executionsErrorSignal.asReadonly();

    readonly jobDetail = this.jobDetailSignal.asReadonly();
    readonly jobDetailLoading = computed(() => this.jobDetailResource.isLoading());
    readonly jobDetailError = this.jobDetailErrorSignal.asReadonly();
    readonly deleteJobLoading = this.deleteJobLoadingSignal.asReadonly();
    readonly deleteJobError = this.deleteJobErrorSignal.asReadonly();

    constructor() {
        queueMicrotask(() => {
            this.refreshJobRegistry();
            this.refreshExecutions();
        });
    }

    refreshExecutions(params: { jobKey?: string; limit?: number } = {}): void {
        this.executionsQuery.set({
            jobKey: params.jobKey?.trim() || undefined,
            limit: typeof params.limit === 'number' ? params.limit : 25,
        });
        this.executionsResource.reload();
    }

    refreshJobRegistry(): void {
        this.jobRegistryResource.reload();
    }

    refreshJobDetail(jobId: string): void {
        const trimmedId = jobId.trim();
        if (!trimmedId) {
            this.jobDetailErrorSignal.set('Job id is required to load job detail.');
            this.jobDetailJobIdSignal.set(null);
            this.jobDetailSignal.set(null);
            return;
        }

        this.jobDetailJobIdSignal.set(trimmedId);
        this.jobDetailResource.reload();
    }

    deleteJob(jobId: string): void {
        const trimmedId = jobId.trim();
        if (!trimmedId) {
            this.deleteJobErrorSignal.set('Job id is required before deleting.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.deleteJobErrorSignal.set('Required context is missing — unable to delete jobs.');
            return;
        }
        if (!environment.trim()) {
            this.deleteJobErrorSignal.set('Environment is not set — select an environment to delete jobs.');
            return;
        }

        this.deleteJobLoadingSignal.set(true);
        this.deleteJobErrorSignal.set(null);

        this.api
            .deleteJob(
                { tenantId, environment, jobId: trimmedId },
                this.tenantContext.createRequestOptions('jobs.delete', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    const current = this.jobDetailSignal();
                    if (current?.jobId === trimmedId || current?.jobKey === trimmedId) {
                        this.jobDetailSignal.set(null);
                    }
                    this.jobRegistrySignal.set(
                        this.jobRegistrySignal().filter((entry) => entry.jobKey !== trimmedId),
                    );
                    this.refreshJobRegistry();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to delete job', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.deleteJobErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.deleteJobErrorSignal.set('Job not found (404) — it may have already been deleted.');
                        return EMPTY;
                    }
                    this.deleteJobErrorSignal.set('Unable to delete job via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.deleteJobLoadingSignal.set(false);
                }),
            )
            .subscribe();
    }

    triggerJob(jobKey: string, metadata: Record<string, string>): void {
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

        this.api
            .triggerJob(
                { jobKey: trimmedKey, metadata },
                this.tenantContext.createRequestOptions(
                    `jobs.trigger:${trimmedKey}`,
                    this.buildCallerOverrides(metadata),
                ),
            )
            .pipe(
                tap(() => {
                    this.updateEntry(entry.id, {
                        status: 'success',
                        completedAt: nowIso(),
                    });
                }),
                catchError((error: unknown) => {
                    console.error('Failed to trigger job', error);
                    this.lastErrorSignal.set('Unable to trigger job via API — entry retained locally.');
                    this.updateEntry(entry.id, {
                        status: 'error',
                        completedAt: nowIso(),
                        error: error instanceof Error ? error.message : 'Unknown error',
                    });
                    return EMPTY;
                }),
            )
            .subscribe();
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
            metadata: { source: 'ui-seed' },
            status: 'success',
            startedAt: isoFromEpochMs(now - 1000 * 60 * 45),
            completedAt: isoFromEpochMs(now - 1000 * 60 * 44),
        },
        {
            id: createEntryId(),
            jobKey: 'webhook-retry',
            metadata: { retries: '3' },
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
