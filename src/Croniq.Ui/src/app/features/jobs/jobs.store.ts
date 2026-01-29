import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import { isoFromEpochMs, nowIso, nowMs } from '@core/time/clock';
import { CRONIQ_API_CLIENT, CallerContext, CroniqApiClient } from 'data-access';
import { EMPTY, catchError, finalize, map, of, tap, forkJoin } from 'rxjs';
import { ExecutionResponse, JobResponse, ScheduleResponse, UpsertJobRequest, UpsertScheduleRequest } from '@croniq/api-schema';

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
    namespace?: string;
    name?: string;
    variant?: string;
    description?: string;
    metadata?: Record<string, string>;
    isActive: boolean;
    assignedRunnerId?: string;
    assignedBy?: string;
    assignedAtUtc?: string;
    assignmentSource?: string;
    assignmentNotes?: string;
    scheduleCount: number;
    activeScheduleCount: number;
    hasDisabledSchedules: boolean;
    managedBy?: string;
    isSeeded: boolean;
    lastExecution?: {
        status: string;
        time: string;
    };
};

export type ExecutionSummary = {
    executionId: string;
    jobKey?: string;
    status?: string;
    startedAt?: string;
    executionMode?: string;
    invocationSource?: string;
    warningType?: string;
    errorMessage?: string;
};

export type JobDetail = {
    jobId: string;
    jobKey?: string;
    description?: string;
    assignedRunnerId?: string;
    assignedBy?: string;
    assignedAtUtc?: string;
    assignmentSource?: string;
    assignmentNotes?: string;
};

@Injectable()
export class JobsStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);

    private readonly triggerLog = signal<ReadonlyArray<ManualTriggerEntry>>(seedManualTriggers());
    private readonly jobRegistrySignal = signal<ReadonlyArray<JobRegistryEntry>>([]);
    private readonly jobRegistryErrorSignal = signal<string | null>(null);
    private readonly jobSchedulesSignal = signal<ReadonlyArray<ScheduleResponse>>([]);

    private readonly jobRegistryResource = tenantRxResource<
        { jobs: JobResponse[]; schedules: ScheduleResponse[]; executions: ExecutionResponse[] },
        { tenantId: string; environment: string }
    >({
        command: 'jobs.list',
        defaultValue: { jobs: [], schedules: [], executions: [] },
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
                this.jobSchedulesSignal.set([]);
                return of({ jobs: [], schedules: [], executions: [] });
            }

            const jobs$ = this.api.listJobs({ tenantId, environment }, requestOptions).pipe(
                map(res => Array.isArray(res) ? res as JobResponse[] : []),
                catchError(() => of([] as JobResponse[]))
            );

            const schedules$ = this.api.getSchedules({ tenantId, environment }, requestOptions).pipe(
                catchError(() => of([] as ScheduleResponse[]))
            );

            // Fetch recent executions to correlate last run status
            const executions$ = this.api.listExecutions({ tenantId, environment, limit: 100 }, requestOptions).pipe(
                map(res => Array.isArray(res) ? res as ExecutionResponse[] : []),
                catchError(() => of([] as ExecutionResponse[]))
            );

            return forkJoin({
                jobs: jobs$,
                schedules: schedules$,
                executions: executions$
            }).pipe(
                map(data => {
                    const normalized = normalizeJobRegistry(data.jobs, data.schedules, data.executions);
                    this.jobRegistrySignal.set(normalized);
                    this.jobSchedulesSignal.set(data.schedules);
                    return data;
                }),
                catchError((error: unknown) => {
                    console.error('Failed to load job registry', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.jobRegistryErrorSignal.set(authFailure.message);
                    } else {
                        this.jobRegistryErrorSignal.set('Unable to load jobs from API.');
                    }
                    this.jobRegistrySignal.set([]);
                    this.jobSchedulesSignal.set([]);
                    return of({ jobs: [], schedules: [], executions: [] });
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

    private readonly toggleSchedulesLoadingSignal = signal(false);
    private readonly toggleSchedulesErrorSignal = signal<string | null>(null);

    private readonly upsertJobLoadingSignal = signal(false);
    private readonly upsertJobErrorSignal = signal<string | null>(null);

    private readonly activateJobLoadingSignal = signal(false);
    private readonly activateJobErrorSignal = signal<string | null>(null);

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
    readonly toggleSchedulesLoading = this.toggleSchedulesLoadingSignal.asReadonly();
    readonly toggleSchedulesError = this.toggleSchedulesErrorSignal.asReadonly();
    readonly upsertJobLoading = this.upsertJobLoadingSignal.asReadonly();
    readonly upsertJobError = this.upsertJobErrorSignal.asReadonly();
    readonly activateJobLoading = this.activateJobLoadingSignal.asReadonly();
    readonly activateJobError = this.activateJobErrorSignal.asReadonly();

    constructor() {
        queueMicrotask(() => {
            this.refreshJobRegistry();
            this.refreshExecutions();
        });
    }

    upsertJob(payload: UpsertJobRequest): void {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.upsertJobErrorSignal.set('Required context is missing — unable to upsert job.');
            return;
        }

        this.upsertJobLoadingSignal.set(true);
        this.upsertJobErrorSignal.set(null);

        this.api
            .upsertJob(
                { tenantId, environment },
                payload,
                this.tenantContext.createRequestOptions('jobs.upsert', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.refreshJobRegistry();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to upsert job', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.upsertJobErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    this.upsertJobErrorSignal.set('Unable to upsert job via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.upsertJobLoadingSignal.set(false);
                }),
            )
            .subscribe();
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

    activateJob(jobKey: string): void {
        const trimmedKey = jobKey.trim();
        if (!trimmedKey) {
            this.activateJobErrorSignal.set('Job key is required to activate a job.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.activateJobErrorSignal.set('Required context is missing — unable to activate job.');
            return;
        }

        this.activateJobLoadingSignal.set(true);
        this.activateJobErrorSignal.set(null);

        this.api
            .activateJob(
                { tenantId, environment, jobId: trimmedKey },
                this.tenantContext.createRequestOptions('jobs.activate', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.refreshJobRegistry();
                    const current = this.jobDetailSignal();
                    if (current?.jobId === trimmedKey || current?.jobKey === trimmedKey) {
                        this.refreshJobDetail(trimmedKey);
                    }
                }),
                catchError((error: unknown) => {
                    console.error('Failed to activate job', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.activateJobErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.activateJobErrorSignal.set('Job not found (404) — it may have been removed.');
                        return EMPTY;
                    }
                    this.activateJobErrorSignal.set('Unable to activate job via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.activateJobLoadingSignal.set(false);
                }),
            )
            .subscribe();
    }

    deactivateJob(job: JobRegistryEntry): void {
        const jobKey = job.jobKey?.trim() ?? '';
        if (!jobKey) {
            this.activateJobErrorSignal.set('Job key is required to deactivate a job.');
            return;
        }

        const identity = resolveJobIdentity(job);
        if (!identity) {
            this.activateJobErrorSignal.set('Job namespace/name is required to deactivate a job.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.activateJobErrorSignal.set('Required context is missing — unable to deactivate job.');
            return;
        }

        const payload: UpsertJobRequest = {
            jobKey,
            namespace: identity.namespace,
            name: identity.name,
            variant: identity.variant,
            description: job.description || undefined,
            metadata: job.metadata || undefined,
            isActive: false,
            assignedRunnerId: job.assignedRunnerId || undefined,
            assignmentNotes: job.assignmentNotes || undefined,
        };

        this.activateJobLoadingSignal.set(true);
        this.activateJobErrorSignal.set(null);

        this.api
            .upsertJob(
                { tenantId, environment },
                payload,
                this.tenantContext.createRequestOptions('jobs.deactivate', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.refreshJobRegistry();
                    const current = this.jobDetailSignal();
                    if (current?.jobId === jobKey || current?.jobKey === jobKey) {
                        this.refreshJobDetail(jobKey);
                    }
                }),
                catchError((error: unknown) => {
                    console.error('Failed to deactivate job', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing jobs permissions.',
                    });
                    if (authFailure) {
                        this.activateJobErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.activateJobErrorSignal.set('Job not found (404) — it may have been removed.');
                        return EMPTY;
                    }
                    this.activateJobErrorSignal.set('Unable to deactivate job via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.activateJobLoadingSignal.set(false);
                }),
            )
            .subscribe();
    }

    setJobSchedulesEnabled(jobKey: string, enabled: boolean): void {
        const trimmedKey = jobKey.trim();
        if (!trimmedKey) {
            this.toggleSchedulesErrorSignal.set('Job key is required to update schedules.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.toggleSchedulesErrorSignal.set('Required context is missing — unable to update schedules.');
            return;
        }

        const schedules = this.jobSchedulesSignal().filter((schedule) => {
            const key = typeof schedule.jobKey === 'string' ? schedule.jobKey.trim() : '';
            return key && key.toLowerCase() === trimmedKey.toLowerCase();
        });

        if (schedules.length === 0) {
            this.toggleSchedulesErrorSignal.set('No schedules found for this job.');
            return;
        }

        const payloads: UpsertScheduleRequest[] = [];
        for (const schedule of schedules) {
            const scheduleJobKey = typeof schedule.jobKey === 'string' ? schedule.jobKey.trim() : '';
            const cronExpression = typeof schedule.cronExpression === 'string' ? schedule.cronExpression.trim() : '';
            if (!scheduleJobKey || !cronExpression) {
                continue;
            }

            payloads.push({
                triggerId: typeof schedule.triggerId === 'string' ? schedule.triggerId : undefined,
                jobKey: scheduleJobKey,
                cronExpression,
                enabled,
                startAtUtc: schedule.startAtUtc ?? undefined,
                endAtUtc: schedule.endAtUtc ?? undefined,
                metadata: schedule.metadata ?? undefined,
                timeZoneId: schedule.timeZoneId ?? undefined,
                calendarId: schedule.calendarId ?? undefined,
            });
        }

        if (payloads.length === 0) {
            this.toggleSchedulesErrorSignal.set('Schedules are missing required data and cannot be updated.');
            return;
        }

        this.toggleSchedulesLoadingSignal.set(true);
        this.toggleSchedulesErrorSignal.set(null);

        const requestOptions = this.tenantContext.createRequestOptions('schedules.upsert', {
            tenantId,
            environment,
        });

        forkJoin(
            payloads.map((payload) => this.api.upsertSchedule({ tenantId, environment }, payload, requestOptions)),
        )
            .pipe(
                tap(() => {
                    this.refreshJobRegistry();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to update schedules', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing schedules permissions.',
                    });
                    if (authFailure) {
                        this.toggleSchedulesErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    this.toggleSchedulesErrorSignal.set('Unable to update schedules via API.');
                    return EMPTY;
                }),
                finalize(() => {
                    this.toggleSchedulesLoadingSignal.set(false);
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

        const registryEntry = findJobEntry(this.jobRegistrySignal(), trimmedKey);
        if (registryEntry && isRunnerRegisteredJob(registryEntry)) {
            this.triggerJobViaSchedule(entry, trimmedKey, metadata);
            return;
        }

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
                    const resolvedMessage = resolveTriggerErrorMessage(error, trimmedKey, this.jobRegistrySignal());
                    this.lastErrorSignal.set(resolvedMessage);
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

    private triggerJobViaSchedule(entry: ManualTriggerEntry, jobKey: string, metadata: Record<string, string>): void {
        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.lastErrorSignal.set('Required context is missing — unable to trigger job.');
            this.updateEntry(entry.id, {
                status: 'error',
                completedAt: nowIso(),
                error: 'Missing tenant context',
            });
            return;
        }

        const payload: UpsertScheduleRequest = {
            jobKey,
            cronExpression: '@once',
            enabled: true,
            startAtUtc: nowIso(),
            metadata: Object.keys(metadata).length > 0 ? metadata : undefined,
        };

        this.api
            .upsertSchedule(
                { tenantId, environment },
                payload,
                this.tenantContext.createRequestOptions('schedules.upsert', {
                    tenantId,
                    environment,
                }),
            )
            .pipe(
                tap(() => {
                    this.updateEntry(entry.id, {
                        status: 'success',
                        completedAt: nowIso(),
                    });
                    this.refreshJobRegistry();
                }),
                catchError((error: unknown) => {
                    console.error('Failed to trigger job via schedule', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) — your token is missing schedules permissions.',
                    });
                    this.lastErrorSignal.set(authFailure?.message ?? 'Unable to trigger job via schedule.');
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

function normalizeJobRegistry(
    jobs: JobResponse[],
    schedules: ScheduleResponse[],
    executions: ExecutionResponse[]
): ReadonlyArray<JobRegistryEntry> {
    if (!Array.isArray(jobs)) {
        return [];
    }

    const entries: JobRegistryEntry[] = [];

    // Index schedules by jobKey
    const scheduleStatsByJob = new Map<string, { total: number; active: number }>();
    for (const schedule of schedules) {
        const key = typeof schedule.jobKey === 'string' ? schedule.jobKey.trim().toLowerCase() : '';
        if (!key) {
            continue;
        }
        const stats = scheduleStatsByJob.get(key) ?? { total: 0, active: 0 };
        stats.total += 1;
        if (schedule.enabled ?? false) {
            stats.active += 1;
        }
        scheduleStatsByJob.set(key, stats);
    }

    // Index latest execution by jobKey
    const latestExecutionByJob = new Map<string, { status: string; time: string }>();
    // Sort executions by time desc first (assuming they might not be sorted)
    const sortedExecutions = [...executions].sort((a, b) => {
        const tA = new Date(a.startedAtUtc || 0).getTime();
        const tB = new Date(b.startedAtUtc || 0).getTime();
        return tB - tA;
    });

    for (const ex of sortedExecutions) {
        if (ex.jobKey && !latestExecutionByJob.has(ex.jobKey)) {
            latestExecutionByJob.set(ex.jobKey, {
                status: mapExecutionStatus(ex.status),
                time: ex.startedAtUtc || ''
            });
        }
    }

    for (const job of jobs) {
        if (!job.jobKey) continue;

        const managedBy = resolveManagedBy(job.metadata ?? undefined);
        const scheduleStats = scheduleStatsByJob.get(job.jobKey.trim().toLowerCase()) ?? { total: 0, active: 0 };
        const totalSchedules = scheduleStats.total;
        const activeSchedules = scheduleStats.active;
        const isActive = typeof job.isActive === 'boolean' ? job.isActive : true;

        entries.push({
            jobKey: job.jobKey,
            namespace: job.namespace || undefined,
            name: job.name || undefined,
            variant: job.variant || undefined,
            description: job.description || undefined,
            metadata: job.metadata || undefined,
            isActive,
            assignedRunnerId: job.assignedRunnerId ?? undefined,
            assignedBy: job.assignedBy ?? undefined,
            assignedAtUtc: job.assignedAtUtc ?? undefined,
            assignmentSource: job.assignmentSource ?? undefined,
            assignmentNotes: job.assignmentNotes ?? undefined,
            scheduleCount: totalSchedules,
            activeScheduleCount: activeSchedules,
            hasDisabledSchedules: activeSchedules < totalSchedules,
            managedBy: managedBy || undefined,
            isSeeded: !!managedBy,
            lastExecution: latestExecutionByJob.get(job.jobKey)
        });
    }

    return entries;
}

function mapExecutionStatus(status: unknown): string {
    if (status === 0 || status === '0' || status === 'Succeeded' || status === 'Success') return 'Success';
    if (status === 1 || status === '1' || status === 'Failed' || status === 'Failure') return 'Failure';
    if (status === 2 || status === '2' || status === 'Canceled' || status === 'Cancelled') return 'Canceled';
    return 'Unknown';
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

        const executionMode =
            typeof record['executionMode'] === 'string' ? record['executionMode'].trim() : undefined;
        const invocationSource =
            typeof record['invocationSource'] === 'string' ? record['invocationSource'].trim() : undefined;
        const warningType = typeof record['errorType'] === 'string' ? record['errorType'].trim() : undefined;
        const errorMessage =
            typeof record['errorMessage'] === 'string' ? record['errorMessage'].trim() : undefined;

        entries.push({
            executionId,
            jobKey: jobKey || undefined,
            status,
            startedAt,
            executionMode: executionMode || undefined,
            invocationSource: invocationSource || undefined,
            warningType: warningType || undefined,
            errorMessage: errorMessage || undefined,
        });
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
    const assignedRunnerId =
        typeof record['assignedRunnerId'] === 'string' ? record['assignedRunnerId'].trim() : undefined;
    const assignedBy = typeof record['assignedBy'] === 'string' ? record['assignedBy'].trim() : undefined;
    const assignedAtUtc =
        typeof record['assignedAtUtc'] === 'string' ? record['assignedAtUtc'].trim() : undefined;
    const assignmentSource =
        typeof record['assignmentSource'] === 'string' ? record['assignmentSource'].trim() : undefined;
    const assignmentNotes =
        typeof record['assignmentNotes'] === 'string' ? record['assignmentNotes'].trim() : undefined;
    return {
        jobId,
        jobKey: jobKey || undefined,
        description: description || undefined,
        assignedRunnerId: assignedRunnerId || undefined,
        assignedBy: assignedBy || undefined,
        assignedAtUtc: assignedAtUtc || undefined,
        assignmentSource: assignmentSource || undefined,
        assignmentNotes: assignmentNotes || undefined,
    };
}

function resolveJobIdentity(job: JobRegistryEntry): { namespace: string; name: string; variant?: string } | null {
    const namespace = job.namespace?.trim() ?? '';
    const name = job.name?.trim() ?? '';
    if (namespace && name) {
        return {
            namespace,
            name,
            variant: job.variant?.trim() || undefined,
        };
    }

    const segments = job.jobKey.split(':').map((segment) => segment.trim()).filter(Boolean);
    if (segments.length < 2 || segments.length > 3) {
        return null;
    }

    return {
        namespace: segments[0],
        name: segments[1],
        variant: segments[2] || undefined,
    };
}

function resolveManagedBy(metadata?: Record<string, string>): string | null {
    if (!metadata) {
        return null;
    }

    for (const [key, value] of Object.entries(metadata)) {
        if (key.trim().toLowerCase() === 'managedby') {
            const trimmed = value?.trim();
            return trimmed ? trimmed : null;
        }
    }

    return null;
}

function resolveTriggerErrorMessage(
    error: unknown,
    jobKey?: string,
    jobRegistry: ReadonlyArray<JobRegistryEntry> = [],
): string {
    if (error instanceof HttpErrorResponse) {
        const payload = error.error as { error?: string; jobKey?: string } | null;
        if (payload?.error === 'job-not-registered') {
            const key = payload.jobKey ?? jobKey ?? 'this job';
            const normalizedKey = key.trim().toLowerCase();
            const match = jobRegistry.find((entry) => entry.jobKey.trim().toLowerCase() === normalizedKey);
            if (match && match.scheduleCount === 0 && isRunnerRegisteredJob(match)) {
                return `Unable to trigger ${key}: this job was registered by a runner and has no schedules. Create an @once schedule (or add a schedule) to run it, or load the job assembly on the API host.`;
            }
            return `Unable to trigger ${key}: the API host has not registered the job. Ensure the job assembly is loaded or JobRegistrySync is enabled.`;
        }
    }

    return 'Unable to trigger job via API - entry retained locally.';
}

function hasMetadataValue(
    metadata: Record<string, string> | undefined,
    key: string,
    expected: string,
): boolean {
    if (!metadata) {
        return false;
    }

    const targetKey = key.trim().toLowerCase();
    const targetValue = expected.trim().toLowerCase();
    for (const [rawKey, rawValue] of Object.entries(metadata)) {
        if (rawKey.trim().toLowerCase() !== targetKey) {
            continue;
        }

        if ((rawValue ?? '').trim().toLowerCase() === targetValue) {
            return true;
        }
    }

    return false;
}

function findJobEntry(
    jobRegistry: ReadonlyArray<JobRegistryEntry>,
    jobKey: string,
): JobRegistryEntry | null {
    const normalizedKey = jobKey.trim().toLowerCase();
    if (!normalizedKey) {
        return null;
    }

    return jobRegistry.find((entry) => entry.jobKey.trim().toLowerCase() === normalizedKey) ?? null;
}

function isRunnerRegisteredJob(job: JobRegistryEntry): boolean {
    if (!job.metadata) {
        return false;
    }

    return hasMetadataValue(job.metadata, 'registrationSource', 'runner')
        || hasMetadataValue(job.metadata, 'createdBy', 'runner');
}
