import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, computed, inject, signal } from '@angular/core';
import { authFailureFromError } from '@core/auth/auth-failure';
import { AuthSessionService } from '@core/auth/auth-session.service';
import { tenantRxResource } from '@core/resource/tenant-rx-resource';
import { RuntimeConfigService } from '@core/runtime-config.service';
import { createSseStream } from '@core/streaming/sse';
import { TenantContextService } from '@core/tenant-context/tenant-context.service';
import type { ExecutionResponse, JobResponse, RunnerListResponse, RunnerStatusModel } from '@croniq/api-schema';
import { CRONIQ_API_CLIENT, CallerContext, type CroniqRequestOptions, CroniqApiClient } from 'data-access';
import { EMPTY, catchError, concatWith, defer, finalize, filter, from, map, of, scan, switchMap, tap, throwError, timer } from 'rxjs';
import { createRunnerPresenceGrpcStream, type RunnerPresenceGrpcStreamRequest } from './runner-presence.grpc';

export interface Runner {
    id: string;
    hostname: string;
    status: 'Online' | 'Offline' | 'Draining';
    lastHeartbeatAt: string;
    activeJobs: number;
    capacity: number;
    tags: string[];
    capabilities: string[];
    runnerInstanceId?: string;
    runnerInstanceLabel: string;
    transportState?: string;
    transportLabel: string;
    allowTestExecutions?: boolean;
    allowTestLabel: string;
    maxInflight?: number;
    maxInflightLabel: string;
    draining: boolean;
    drainLabel: string;
    loadPercent: number;
    loadLabel: string;
    recentJobs: ReadonlyArray<string>;
    assignedJobs: ReadonlyArray<string>;
}

interface RunnerMetadata {
    hostname?: string;
    tags?: string[];
    capacity?: number;
    activeJobs?: number;
    runnerInstanceId?: string;
    transportState?: string;
    allowTestExecutions?: boolean;
    maxInflight?: number;
    draining?: boolean;
    capabilities?: string[];
}

const DEFAULT_LOAD_LABEL = 'n/a';
const DEFAULT_VALUE_LABEL = '--';
const MAX_RECENT_JOBS = 4;
type RunnerPresenceStreamMode = 'grpc' | 'sse' | 'polling';

const RUNNER_PRESENCE_STREAM_COMMAND = 'runners.presence.stream';
const RUNNER_PRESENCE_POLL_INTERVAL_MS = 10_000;

const parseRunnerMetadata = (metadataJson?: string | null): RunnerMetadata => {
    if (!metadataJson) {
        return {};
    }

    try {
        const parsed: unknown = JSON.parse(metadataJson);
        if (!parsed || typeof parsed !== 'object') {
            return {};
        }

        const record = parsed as Record<string, unknown>;
        const hostnameValue = record['hostname'];
        const tagsValue = record['tags'];
        const capacityValue = record['capacity'];
        const activeJobsValue = record['activeJobs'];
        const runnerInstanceValue = record['runnerInstanceId'];
        const transportStateValue = record['transportState'];
        const allowTestValue = record['allowTestExecutions'];
        const maxInflightValue = record['maxInflight'];
        const drainingValue = record['draining'];
        const drainStatusValue = record['drainStatus'];
        const capabilitiesValue = record['capabilities'];

        const hostname = typeof hostnameValue === 'string' ? hostnameValue : undefined;
        const tags = Array.isArray(tagsValue)
            ? tagsValue.filter((tag): tag is string => typeof tag === 'string')
            : undefined;
        const capacity = typeof capacityValue === 'number' ? capacityValue : undefined;
        const activeJobs = typeof activeJobsValue === 'number' ? activeJobsValue : undefined;
        const runnerInstanceId = typeof runnerInstanceValue === 'string' ? runnerInstanceValue : undefined;
        const transportState = typeof transportStateValue === 'string' ? transportStateValue : undefined;
        const allowTestExecutions = typeof allowTestValue === 'boolean' ? allowTestValue : undefined;
        const maxInflight = typeof maxInflightValue === 'number' ? maxInflightValue : undefined;
        const capabilities = Array.isArray(capabilitiesValue)
            ? capabilitiesValue.filter((tag): tag is string => typeof tag === 'string')
            : undefined;
        const draining = typeof drainingValue === 'boolean'
            ? drainingValue
            : typeof drainStatusValue === 'string'
                ? drainStatusValue.toLowerCase() === 'draining'
                : typeof drainStatusValue === 'boolean'
                    ? drainStatusValue
                    : undefined;

        return {
            hostname,
            tags,
            capacity,
            activeJobs,
            runnerInstanceId,
            transportState,
            allowTestExecutions,
            maxInflight,
            draining,
            capabilities,
        };
    } catch {
        return {};
    }
};

const buildRunnerTags = (metadata: RunnerMetadata): string[] => {
    const tags = metadata.tags ?? [];
    const capabilities = metadata.capabilities ?? [];
    if (tags.length === 0 && capabilities.length === 0) {
        return [];
    }
    return Array.from(new Set([...tags, ...capabilities])).sort((a, b) => a.localeCompare(b));
};

const toTransportLabel = (transport?: string): string => {
    if (!transport) {
        return DEFAULT_VALUE_LABEL;
    }
    const normalized = transport.trim().toLowerCase();
    if (normalized === 'grpc') {
        return 'gRPC';
    }
    if (normalized === 'polling') {
        return 'Polling';
    }
    if (normalized === 'disconnected') {
        return 'Disconnected';
    }
    return transport;
};

const toPresenceTransportLabel = (transport: RunnerPresenceStreamMode): string => {
    if (transport === 'grpc') {
        return 'gRPC stream';
    }
    if (transport === 'sse') {
        return 'SSE stream';
    }
    return 'Polling';
};

const toAllowTestLabel = (allowTest?: boolean): string => {
    if (allowTest === undefined) {
        return DEFAULT_VALUE_LABEL;
    }
    return allowTest ? 'Allowed' : 'Blocked';
};

const toMaxInflightLabel = (maxInflight?: number): string => {
    if (!maxInflight || maxInflight <= 0) {
        return DEFAULT_VALUE_LABEL;
    }
    return maxInflight.toString();
};

const mapRunnerStatus = (runner: RunnerStatusModel): Runner => {
    const metadata = parseRunnerMetadata(runner.metadataJson);
    const capacity = metadata.capacity ?? metadata.maxInflight ?? 0;
    const activeJobs = metadata.activeJobs ?? 0;
    const loadPercent = capacity > 0 ? Math.min(100, (activeJobs / capacity) * 100) : 0;
    const loadLabel = capacity > 0 ? `${activeJobs}/${capacity}` : DEFAULT_LOAD_LABEL;
    const tags = buildRunnerTags(metadata);
    const draining = metadata.draining === true;
    const status = runner.isOnline ? (draining ? 'Draining' : 'Online') : 'Offline';
    const drainLabel = runner.isOnline
        ? (draining ? 'Draining' : 'Accepting work')
        : 'Offline';
    const maxInflight = metadata.maxInflight ?? metadata.capacity;

    return {
        id: runner.runnerId,
        hostname: metadata.hostname ?? runner.runnerId,
        status,
        lastHeartbeatAt: runner.lastSeenAtUtc ?? '',
        activeJobs,
        capacity,
        tags,
        capabilities: metadata.capabilities ?? [],
        runnerInstanceId: metadata.runnerInstanceId,
        runnerInstanceLabel: metadata.runnerInstanceId ?? DEFAULT_VALUE_LABEL,
        transportState: metadata.transportState,
        transportLabel: toTransportLabel(metadata.transportState),
        allowTestExecutions: metadata.allowTestExecutions,
        allowTestLabel: toAllowTestLabel(metadata.allowTestExecutions),
        maxInflight,
        maxInflightLabel: toMaxInflightLabel(maxInflight),
        draining,
        drainLabel,
        loadPercent,
        loadLabel,
        recentJobs: [],
        assignedJobs: [],
    };
}

const toEpochMs = (value?: string | null): number => {
    if (!value) {
        return 0;
    }

    const parsed = Date.parse(value);
    return Number.isFinite(parsed) ? parsed : 0;
};

const buildRecentJobsByRunner = (executions: ExecutionResponse[]): Map<string, string[]> => {
    const sorted = [...executions].sort((a, b) => toEpochMs(b.startedAtUtc) - toEpochMs(a.startedAtUtc));
    const results = new Map<string, string[]>();

    for (const exec of sorted) {
        const runnerId = typeof exec.instanceId === 'string' ? exec.instanceId.trim() : '';
        const jobKey = typeof exec.jobKey === 'string' ? exec.jobKey.trim() : '';
        if (!runnerId || !jobKey) {
            continue;
        }

        const current = results.get(runnerId) ?? [];
        if (current.includes(jobKey)) {
            continue;
        }

        current.push(jobKey);
        results.set(runnerId, current);
    }

    for (const [runnerId, jobs] of results.entries()) {
        if (jobs.length > MAX_RECENT_JOBS) {
            results.set(runnerId, jobs.slice(0, MAX_RECENT_JOBS));
        }
    }

    return results;
};

const buildAssignedJobsByRunner = (jobs: JobResponse[]): Map<string, string[]> => {
    const results = new Map<string, string[]>();

    for (const job of jobs) {
        const runnerId = typeof job.assignedRunnerId === 'string' ? job.assignedRunnerId.trim() : '';
        const jobKey = typeof job.jobKey === 'string' ? job.jobKey.trim() : '';
        if (!runnerId || !jobKey) {
            continue;
        }

        const list = results.get(runnerId) ?? [];
        if (!list.includes(jobKey)) {
            list.push(jobKey);
        }
        results.set(runnerId, list);
    }

    for (const [runnerId, list] of results.entries()) {
        results.set(runnerId, list.sort((a, b) => a.localeCompare(b)));
    }

    return results;
};

@Injectable()
export class RunnersStore {
    private readonly api = inject<CroniqApiClient>(CRONIQ_API_CLIENT);
    private readonly tenantContext = inject(TenantContextService);
    private readonly authSession = inject(AuthSessionService);
    private readonly runtimeConfig = inject(RuntimeConfigService);

    private readonly actionErrorSignal = signal<string | null>(null);
    private readonly actionLoadingSignal = signal(false);
    private readonly presenceTransportSignal = signal<RunnerPresenceStreamMode>('polling');

    readonly actionError = this.actionErrorSignal.asReadonly();
    readonly actionLoading = this.actionLoadingSignal.asReadonly();
    readonly presenceTransportLabel = computed(() => toPresenceTransportLabel(this.presenceTransportSignal()));

    readonly runnersResource = tenantRxResource<Runner[], { tenantId: string; environment: string }>({
        command: 'runners.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions, abortSignal }) => {
            const { tenantId, environment } = params;
            if (!tenantId) return of([]);

            const requestContext = requestOptions.context
                ?? this.tenantContext.createCallerContext(RUNNER_PRESENCE_STREAM_COMMAND, {
                    tenantId,
                    environment,
                });
            return createRunnerPresenceRunnerStream({
                api: this.api,
                requestOptions,
                tenantId,
                environment,
                includeOffline: true,
                streamMode: this.runtimeConfig.runnersPresenceStreamMode,
                grpcBaseUrl: this.runtimeConfig.runnersPresenceGrpcBaseUrl,
                sseBaseUrl: this.runtimeConfig.runnersPresenceSseBaseUrl,
                requestContext,
                sessionToken: this.authSession.getSessionToken(),
                abortSignal,
                onTransport: (transport) => this.presenceTransportSignal.set(transport),
            });
        }
    });

    readonly executionsResource = tenantRxResource<ExecutionResponse[], { tenantId: string; environment: string }>({
        command: 'executions.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const { tenantId, environment } = params;
            if (!tenantId) {
                return of([] as ExecutionResponse[]);
            }

            return this.api.listExecutions({ tenantId, environment, limit: 200 }, requestOptions).pipe(
                map((response) => (Array.isArray(response) ? response as ExecutionResponse[] : [])),
                catchError((err) => {
                    console.error('Failed to load executions for runner jobs', err);
                    return of([] as ExecutionResponse[]);
                }),
            );
        },
    });

    readonly jobsResource = tenantRxResource<JobResponse[], { tenantId: string; environment: string }>({
        command: 'jobs.list',
        defaultValue: [],
        params: () => {
            const { tenantId, environment } = this.tenantContext.snapshot();
            return { tenantId, environment };
        },
        stream: ({ params, requestOptions }) => {
            const { tenantId, environment } = params;
            if (!tenantId) {
                return of([] as JobResponse[]);
            }

            return this.api.listJobs({ tenantId, environment }, requestOptions).pipe(
                map((response) => (Array.isArray(response) ? response as JobResponse[] : [])),
                catchError((err) => {
                    console.error('Failed to load assigned jobs', err);
                    return of([] as JobResponse[]);
                }),
            );
        },
    });

    readonly runners = computed(() => {
        const runners = this.runnersResource.value() ?? [];
        const executions = this.executionsResource.value() ?? [];
        const jobs = this.jobsResource.value() ?? [];
        const jobsByRunner = buildRecentJobsByRunner(executions);
        const assignedJobsByRunner = buildAssignedJobsByRunner(jobs);

        return runners.map((runner) => ({
            ...runner,
            recentJobs: jobsByRunner.get(runner.id) ?? [],
            assignedJobs: assignedJobsByRunner.get(runner.id) ?? [],
        }));
    });
    readonly loading = computed(() => this.runnersResource.isLoading());
    readonly error = computed(() => this.runnersResource.error());

    // Metrics
    readonly activeRunnersCount = computed(() => this.runners().filter(r => r.status === 'Online').length);
    readonly totalCapacity = computed(() => this.runners().reduce((acc, r) => acc + (r.capacity || 0), 0));
    readonly busyThreads = computed(() => this.runners().reduce((acc, r) => acc + (r.activeJobs || 0), 0));

    refresh() {
        this.runnersResource.reload();
        this.jobsResource.reload();
        this.executionsResource.reload();
    }

    drainRunner(runnerId: string, draining = true): void {
        const trimmedId = runnerId.trim();
        if (!trimmedId) {
            this.actionErrorSignal.set('Runner id is required before draining.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.actionErrorSignal.set('Required context is missing - unable to drain runner.');
            return;
        }

        this.actionLoadingSignal.set(true);
        this.actionErrorSignal.set(null);

        this.api
            .drainRunner(
                { tenantId, environment, runnerId: trimmedId },
                { environmentTag: environment, draining },
                this.tenantContext.createRequestOptions('runners.drain', { tenantId, environment }),
            )
            .pipe(
                tap(() => this.refresh()),
                catchError((error: unknown) => {
                    console.error('Failed to drain runner', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing runners permissions.',
                    });
                    if (authFailure) {
                        this.actionErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.actionErrorSignal.set('Runner not found (404) - it may have already gone offline.');
                        return EMPTY;
                    }
                    this.actionErrorSignal.set('Unable to drain runner via API.');
                    return EMPTY;
                }),
                finalize(() => this.actionLoadingSignal.set(false)),
            )
            .subscribe();
    }

    deregisterRunner(runnerId: string): void {
        const trimmedId = runnerId.trim();
        if (!trimmedId) {
            this.actionErrorSignal.set('Runner id is required before deregistering.');
            return;
        }

        const { tenantId, environment } = this.tenantContext.snapshot();
        if (!tenantId.trim()) {
            this.actionErrorSignal.set('Required context is missing - unable to deregister runner.');
            return;
        }

        this.actionLoadingSignal.set(true);
        this.actionErrorSignal.set(null);

        this.api
            .deregisterRunner(
                { tenantId, environment, runnerId: trimmedId },
                this.tenantContext.createRequestOptions('runners.deregister', { tenantId, environment }),
            )
            .pipe(
                tap(() => this.refresh()),
                catchError((error: unknown) => {
                    console.error('Failed to deregister runner', error);
                    const authFailure = authFailureFromError(error, {
                        forbidden: 'Forbidden (403) - your token is missing runners permissions.',
                    });
                    if (authFailure) {
                        this.actionErrorSignal.set(authFailure.message);
                        return EMPTY;
                    }
                    if (error instanceof HttpErrorResponse && error.status === 404) {
                        this.actionErrorSignal.set('Runner not found (404) - it may have already been removed.');
                        return EMPTY;
                    }
                    this.actionErrorSignal.set('Unable to deregister runner via API.');
                    return EMPTY;
                }),
                finalize(() => this.actionLoadingSignal.set(false)),
            )
            .subscribe();
    }
}

type RunnerPresenceStreamContext = {
    api: CroniqApiClient;
    requestOptions: CroniqRequestOptions;
    tenantId: string;
    environment: string;
    includeOffline: boolean;
    streamMode: RunnerPresenceStreamMode;
    grpcBaseUrl: string;
    sseBaseUrl: string;
    requestContext: CallerContext;
    sessionToken: string | null;
    abortSignal: AbortSignal;
    onTransport?: (transport: RunnerPresenceStreamMode) => void;
};

type RunnerPresenceDeltaEvent = {
    type?: string;
    snapshot: RunnerStatusModel[];
    updated: RunnerStatusModel[];
    removedRunnerIds: string[];
};

function createRunnerPresenceRunnerStream(context: RunnerPresenceStreamContext) {
    const onTransport = context.onTransport ?? (() => undefined);
    const poll$ = withTransport('polling', createRunnerListPollingStream(context), onTransport);
    const sse$ = withTransport(
        'sse',
        createRunnerPresenceDeltaStream(context, createSsePresenceEventStream).pipe(
            catchError((error: unknown) => {
                console.warn('Runner presence SSE stream unavailable; falling back to polling.', error);
                return poll$;
            }),
        ),
        onTransport,
    );

    if (context.streamMode === 'polling') {
        return poll$;
    }

    if (context.streamMode === 'sse') {
        return sse$;
    }

    return withTransport(
        'grpc',
        createRunnerPresenceDeltaStream(context, createGrpcPresenceEventStream).pipe(
            catchError((error: unknown) => {
                console.warn('Runner presence gRPC stream unavailable; falling back to SSE.', error);
                return sse$;
            }),
        ),
        onTransport,
    );
}

function withTransport(
    transport: RunnerPresenceStreamMode,
    source$: ReturnType<typeof createRunnerListPollingStream>,
    onTransport: (transport: RunnerPresenceStreamMode) => void,
) {
    return defer(() => {
        onTransport(transport);
        return source$;
    });
}

function createRunnerListPollingStream(context: RunnerPresenceStreamContext) {
    const request = () =>
        context.api.listRunners(
            { tenantId: context.tenantId, environment: context.environment, includeOffline: context.includeOffline },
            context.requestOptions,
        ).pipe(
            map((response: RunnerListResponse) => (response.runners ?? []).map(mapRunnerStatus)),
            catchError((err) => {
                console.error('Failed to load runners', err);
                return of<Runner[]>([]);
            }),
        );

    if (RUNNER_PRESENCE_POLL_INTERVAL_MS <= 0) {
        return request();
    }

    return timer(0, RUNNER_PRESENCE_POLL_INTERVAL_MS).pipe(
        switchMap(() => request()),
    );
}

function createRunnerPresenceDeltaStream(
    context: RunnerPresenceStreamContext,
    createEventStream: (context: RunnerPresenceStreamContext) => ReturnType<typeof createGrpcPresenceEventStream>,
) {
    return createEventStream(context).pipe(
        map((event) => normalizeRunnerPresenceEvent(event)),
        map((event) => ensureDeltaEvent(event)),
        filter((event) => shouldApplyPresenceEvent(event)),
        scan((state, event) => applyRunnerPresenceDelta(state, event), new Map<string, Runner>()),
        map((state) => mapRunnerPresenceState(state)),
    );
}

function createGrpcPresenceEventStream(context: RunnerPresenceStreamContext) {
    if (!context.grpcBaseUrl) {
        return throwError(() => new Error('gRPC base URL not configured.'));
    }

    return defer(() => {
        const streamContext: CallerContext = {
            ...context.requestContext,
            command: RUNNER_PRESENCE_STREAM_COMMAND,
        };
        const headers = buildStreamHeaders(streamContext, context.sessionToken);
        const request = buildGrpcPresenceStreamRequest(context);
        const stream = createRunnerPresenceGrpcStream(request, {
            baseUrl: context.grpcBaseUrl,
            headers,
            signal: context.abortSignal,
        });

        return from(stream).pipe(
            map((event) => event),
            concatWith(throwError(() => new Error('gRPC stream closed unexpectedly.'))),
        );
    });
}

function createSsePresenceEventStream(context: RunnerPresenceStreamContext) {
    const url = buildPresenceStreamUrl(
        context.sseBaseUrl,
        context.tenantId,
        context.environment,
        context.includeOffline,
    );
    if (!url) {
        return throwError(() => new Error('SSE base URL not configured.'));
    }

    const streamContext: CallerContext = {
        ...context.requestContext,
        command: RUNNER_PRESENCE_STREAM_COMMAND,
    };
    const headers = buildStreamHeaders(streamContext, context.sessionToken);
    return createSseStream(url, { headers, signal: context.abortSignal }).pipe(
        map((event) => parseSseRunnerPresenceEvent(event.data)),
    );
}

function buildGrpcPresenceStreamRequest(context: RunnerPresenceStreamContext): RunnerPresenceGrpcStreamRequest {
    return {
        tenantId: context.tenantId,
        environmentTag: context.environment,
        includeOffline: context.includeOffline,
    };
}

function buildPresenceStreamUrl(
    baseUrl: string,
    tenantId: string,
    environment: string,
    includeOffline: boolean,
): string | null {
    const normalizedBase = baseUrl?.trim();
    if (!normalizedBase) {
        return null;
    }

    const params = new URLSearchParams();
    if (environment) {
        params.set('environment', environment);
    }
    if (includeOffline) {
        params.set('includeOffline', 'true');
    }

    const path = `/tenants/${encodeURIComponent(tenantId)}/runners/stream`;
    const queryString = params.toString();
    const relative = queryString ? `${path}?${queryString}` : path;

    if (normalizedBase.startsWith('/')) {
        return `${normalizedBase}${relative}`;
    }

    try {
        return new URL(relative, normalizedBase).toString();
    } catch {
        return null;
    }
}

function buildStreamHeaders(context: CallerContext, sessionToken: string | null): Record<string, string> {
    const headers: Record<string, string> = {
        'X-Croniq-Client': 'Croniq.Ui',
        'Cache-Control': 'no-store, no-cache, max-age=0',
        Pragma: 'no-cache',
    };

    if (context.source) {
        headers['X-Croniq-Source'] = context.source;
    }
    if (context.actor) {
        headers['X-Croniq-Actor'] = context.actor;
    }
    if (context.tenantId) {
        headers['X-Croniq-Tenant'] = context.tenantId;
    }
    if (context.environment) {
        headers['X-Croniq-Environment'] = context.environment;
    }
    if (context.command) {
        headers['X-Croniq-Command'] = context.command;
    }

    if (sessionToken) {
        headers['Authorization'] = `Bearer ${sessionToken}`;
    }

    return headers;
}

function parseSseRunnerPresenceEvent(data: string): unknown {
    if (!data) {
        return null;
    }

    try {
        return JSON.parse(data) as unknown;
    } catch (error) {
        console.warn('Runner presence SSE event parse failed.', error);
        return null;
    }
}

function normalizeRunnerPresenceEvent(raw: unknown): RunnerPresenceDeltaEvent | null {
    if (!raw || typeof raw !== 'object') {
        return null;
    }

    const record = raw as Record<string, unknown>;
    const typeValue = record['type'];
    const type = typeof typeValue === 'string' ? typeValue : undefined;
    const snapshot = normalizeRunnerStatusList(record['snapshot']);
    const updated = normalizeRunnerStatusList(record['updated']);
    const removedRunnerIds = normalizeStringList(record['removedRunnerIds'] ?? record['removed_runner_ids']);

    return {
        type,
        snapshot,
        updated,
        removedRunnerIds,
    };
}

function normalizeRunnerStatusList(value: unknown): RunnerStatusModel[] {
    if (!Array.isArray(value)) {
        return [];
    }

    const entries: RunnerStatusModel[] = [];
    for (const item of value) {
        const normalized = normalizeRunnerStatus(item);
        if (normalized) {
            entries.push(normalized);
        }
    }
    return entries;
}

function normalizeRunnerStatus(value: unknown): RunnerStatusModel | null {
    if (!value || typeof value !== 'object') {
        return null;
    }

    const record = value as Record<string, unknown>;
    const runnerId = normalizeString(record['runnerId'] ?? record['runner_id']);
    if (!runnerId) {
        return null;
    }

    const lastSeenAtUtc = normalizeString(record['lastSeenAtUtc'] ?? record['last_seen_at_utc']) ?? '';
    const expiresAtUtc = normalizeString(record['expiresAtUtc'] ?? record['expires_at_utc']) ?? '';
    const metadataJson = normalizeString(record['metadataJson'] ?? record['metadata_json']);
    const isOnline = normalizeBoolean(record['isOnline'] ?? record['is_online']) ?? false;

    return {
        runnerId,
        lastSeenAtUtc,
        expiresAtUtc,
        isOnline,
        metadataJson,
    };
}

function normalizeStringList(value: unknown): string[] {
    if (!Array.isArray(value)) {
        return [];
    }
    return value
        .map((entry) => normalizeString(entry))
        .filter((entry): entry is string => !!entry);
}

function normalizeString(value: unknown): string | null {
    if (typeof value !== 'string') {
        return null;
    }
    const trimmed = value.trim();
    return trimmed ? trimmed : null;
}

function normalizeBoolean(value: unknown): boolean | null {
    if (typeof value === 'boolean') {
        return value;
    }
    return null;
}

function ensureDeltaEvent(event: RunnerPresenceDeltaEvent | null): RunnerPresenceDeltaEvent {
    if (!event) {
        throw new Error('Runner presence stream event missing payload.');
    }

    const hasChanges = event.snapshot.length > 0 || event.updated.length > 0 || event.removedRunnerIds.length > 0;
    if (!hasChanges && event.type && event.type.toLowerCase() === 'presence.updated') {
        throw new Error('Runner presence stream does not provide deltas.');
    }

    return event;
}

function shouldApplyPresenceEvent(event: RunnerPresenceDeltaEvent): boolean {
    if (event.snapshot.length > 0 || event.updated.length > 0 || event.removedRunnerIds.length > 0) {
        return true;
    }

    return event.type?.toLowerCase() === 'presence.snapshot';
}

function applyRunnerPresenceDelta(
    state: Map<string, Runner>,
    event: RunnerPresenceDeltaEvent,
): Map<string, Runner> {
    const isSnapshot = event.snapshot.length > 0 || event.type?.toLowerCase() === 'presence.snapshot';
    if (isSnapshot) {
        const fresh = new Map<string, Runner>();
        event.snapshot.forEach((runner) => {
            const mapped = mapRunnerStatus(runner);
            fresh.set(mapped.id, mapped);
        });
        return fresh;
    }

    if (event.updated.length === 0 && event.removedRunnerIds.length === 0) {
        return state;
    }

    const next = new Map(state);
    event.updated.forEach((runner) => {
        const mapped = mapRunnerStatus(runner);
        next.set(mapped.id, mapped);
    });
    event.removedRunnerIds.forEach((runnerId) => {
        next.delete(runnerId);
    });

    return next;
}

function mapRunnerPresenceState(state: Map<string, Runner>): Runner[] {
    const list = Array.from(state.values());
    return list.sort((a, b) => a.id.localeCompare(b.id));
}
