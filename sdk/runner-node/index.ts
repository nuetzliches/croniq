import * as grpc from '@grpc/grpc-js';
import * as protoLoader from '@grpc/proto-loader';
import { randomUUID } from 'crypto';
import { EventEmitter } from 'events';
import { promises as fs } from 'fs';
import path from 'path';

export type Lease = {
    executionId: string;
    leaseId: string;
    triggerId: string;
    jobKey: string;
    fireAtUtc: string;
    leaseExpiresAtUtc: string;
    payload?: string | null;
    executionMode?: string;
    invocationSource?: string;
};

export type PollRequest = {
    runnerId: string;
    batchSize?: number;
    waitForMs?: number;
    allowTestExecutions?: boolean;
    maxInflight?: number;
    capabilities?: string[];
};

export type RenewRequest = {
    runnerId: string;
    lease: Lease;
};

export type AckRequest = {
    runnerId: string;
    lease: Lease;
    succeeded: boolean;
    nextFireTimeUtc?: string;
    deadLetterReason?: string;
};

export type WorkEvent = {
    message: string;
    level?: string;
    timestampUtc?: string;
    properties?: Record<string, string>;
    eventType?: string;
};

export type EventsRequest = {
    runnerId: string;
    lease: Lease;
    events: WorkEvent[];
};

export type HeartbeatRequest = {
    runnerId: string;
    environmentTag?: string;
    seenAtUtc?: string;
    metadataJson?: string;
};

export type RunnerClientConfig = {
    baseUrl: string;
    tenantId: string;
    environment?: string;
    apiKey?: string;
    bearerToken?: string;
    fetchImpl?: typeof fetch;
};

export type TransportMode = 'auto' | 'grpc' | 'polling';

export type RunnerConfig = {
    baseUrl: string;
    grpcBaseUrl?: string;
    tenantId: string;
    environment?: string;
    apiKey?: string;
    bearerToken?: string;
    runnerId: string;
    transportMode?: TransportMode;
    allowTestExecutions?: boolean;
    maxInflight?: number;
    capabilities?: string[];
    pollBatchSize?: number;
    pollWaitMs?: number;
    requestTimeoutMs?: number;
    renewLeadMs?: number;
    retryBaseMs?: number;
    retryMaxMs?: number;
    retryMaxAttempts?: number;
    heartbeatIntervalMs?: number;
    heartbeatMetadata?: Record<string, unknown>;
    parsePayloadJson?: boolean;
    outboxPath?: string;
    outboxMaxEntries?: number;
    outboxMaxBytes?: number;
    fetchImpl?: typeof fetch;
};

export type RunnerExecutionContext = {
    executionId: string;
    leaseId: string;
    triggerId: string;
    jobKey: string;
    fireAtUtc: string;
    leaseExpiresAtUtc: string;
    executionMode?: string;
    invocationSource?: string;
    emitEvent?: (event: WorkEvent) => Promise<void>;
};

export type RunnerLogger = {
    info(message: string, data?: Record<string, unknown>): void;
    warn(message: string, data?: Record<string, unknown>): void;
    error(message: string, data?: Record<string, unknown>): void;
};

export function loadRunnerConfigFromEnv(env: Record<string, string | undefined> = process.env): RunnerConfig {
    const baseUrl = env.CRONIQ_API_BASEURL?.trim();
    const grpcBaseUrl = env.CRONIQ_GRPC_BASEURL?.trim();
    const tenantId = env.CRONIQ_TENANT_ID?.trim();
    const environment = env.CRONIQ_ENVIRONMENT?.trim();
    const apiKey = env.CRONIQ_API_KEY?.trim();
    const bearerToken = env.CRONIQ_BEARER_TOKEN?.trim();
    const runnerId = env.CRONIQ_RUNNER_ID?.trim();
    const transportMode = (env.CRONIQ_TRANSPORT_MODE?.trim().toLowerCase() || 'auto') as TransportMode;

    if (!baseUrl) {
        throw new Error('CRONIQ_API_BASEURL is required');
    }
    if (!tenantId) {
        throw new Error('CRONIQ_TENANT_ID is required');
    }
    if (!environment) {
        throw new Error('CRONIQ_ENVIRONMENT is required');
    }
    if (!runnerId) {
        throw new Error('CRONIQ_RUNNER_ID is required');
    }
    if ((!!apiKey && !!bearerToken) || (!apiKey && !bearerToken)) {
        throw new Error('Set exactly one of CRONIQ_API_KEY or CRONIQ_BEARER_TOKEN');
    }
    if (!['auto', 'grpc', 'polling'].includes(transportMode)) {
        throw new Error('CRONIQ_TRANSPORT_MODE must be auto, grpc, or polling');
    }

    return {
        baseUrl,
        grpcBaseUrl,
        tenantId,
        environment,
        apiKey,
        bearerToken,
        runnerId,
        transportMode,
        allowTestExecutions: parseBool(env.CRONIQ_ALLOW_TEST_EXECUTIONS),
        maxInflight: parseNumber(env.CRONIQ_MAX_INFLIGHT),
        pollBatchSize: parseNumber(env.CRONIQ_POLL_BATCH_SIZE),
        pollWaitMs: parseNumber(env.CRONIQ_POLL_WAIT_MS),
        requestTimeoutMs: parseNumber(env.CRONIQ_REQUEST_TIMEOUT_MS),
        renewLeadMs: parseNumber(env.CRONIQ_RENEW_LEAD_MS),
        retryBaseMs: parseNumber(env.CRONIQ_RETRY_BASE_MS),
        retryMaxMs: parseNumber(env.CRONIQ_RETRY_MAX_MS),
        retryMaxAttempts: parseNumber(env.CRONIQ_RETRY_MAX_ATTEMPTS),
        capabilities: parseList(env.CRONIQ_CAPABILITIES),
    };
}

function parseBool(value?: string): boolean | undefined {
    if (value === undefined) {
        return undefined;
    }
    const normalized = value.trim().toLowerCase();
    if (normalized === 'true' || normalized === '1') {
        return true;
    }
    if (normalized === 'false' || normalized === '0') {
        return false;
    }
    throw new Error(`Invalid boolean value: ${value}`);
}

function parseNumber(value?: string): number | undefined {
    if (value === undefined) {
        return undefined;
    }
    const parsed = Number(value);
    if (Number.isNaN(parsed)) {
        throw new Error(`Invalid numeric value: ${value}`);
    }
    return parsed;
}

function parseList(value?: string): string[] | undefined {
    if (!value) {
        return undefined;
    }
    const items = value
        .split(',')
        .map((entry) => entry.trim())
        .filter(Boolean);
    return items.length > 0 ? items : undefined;
}

function isRunnerMismatchResponse(json: unknown, bodyText: string): boolean {
    const payload = json && typeof json === 'object' ? (json as Record<string, unknown>) : undefined;
    const title = payload?.title ?? payload?.error;
    if (typeof title === 'string' && title.toLowerCase() === 'runner-mismatch') {
        return true;
    }
    return bodyText.toLowerCase().includes('runner-mismatch');
}

function isGrpcRunnerMismatch(err: unknown): boolean {
    if (err instanceof RunnerMismatchError) {
        return true;
    }
    const candidate = err as { code?: number; details?: string } | null;
    if (!candidate) {
        return false;
    }
    if (candidate.code !== grpc.status.PERMISSION_DENIED) {
        return false;
    }
    return (candidate.details ?? '').toLowerCase().includes('runner-mismatch');
}

export type RunnerExecuteHandler = (
    context: RunnerExecutionContext,
    payload: unknown,
    logger: RunnerLogger,
) => Promise<void>;

export class CroniqError extends Error {
    status: number;
    body: string;

    constructor(message: string, status: number, body: string) {
        super(message);
        this.name = 'CroniqError';
        this.status = status;
        this.body = body;
    }
}

export class LeaseConflictError extends CroniqError {
    constructor(body: string) {
        super('lease conflict', 409, body);
        this.name = 'LeaseConflictError';
    }
}

export class LeaseNotFoundError extends CroniqError {
    constructor(body: string) {
        super('lease not found', 404, body);
        this.name = 'LeaseNotFoundError';
    }
}

export class RunnerMismatchError extends CroniqError {
    constructor(body: string) {
        super('runner mismatch', 403, body);
        this.name = 'RunnerMismatchError';
    }
}

type PostResult<T> = {
    status: number;
    json: T | null;
};

export class RunnerClient {
    private baseUrl: string;
    private tenantId: string;
    private environment: string;
    private apiKey: string;
    private bearerToken: string;
    private fetchImpl: typeof fetch;

    constructor({
        baseUrl,
        tenantId,
        environment,
        apiKey,
        bearerToken,
        fetchImpl,
    }: RunnerClientConfig) {
        if (!baseUrl) {
            throw new Error('baseUrl is required');
        }
        if (!tenantId) {
            throw new Error('tenantId is required');
        }
        const hasApiKey = !!apiKey;
        const hasBearerToken = !!bearerToken;
        if (hasApiKey === hasBearerToken) {
            throw new Error('apiKey or bearerToken is required (but not both)');
        }

        const resolvedFetch = fetchImpl ?? globalThis.fetch;
        if (!resolvedFetch) {
            throw new Error('fetch is not available; provide fetchImpl');
        }

        this.baseUrl = baseUrl.replace(/\/+$/, '');
        this.tenantId = tenantId;
        this.environment = environment || '';
        this.apiKey = apiKey || '';
        this.bearerToken = bearerToken || '';
        this.fetchImpl = resolvedFetch.bind(globalThis);
    }

    async poll({ runnerId, batchSize = 1, waitForMs = 0, allowTestExecutions, maxInflight, capabilities }: PollRequest): Promise<Lease[]> {
        const body = { runnerId, batchSize, waitForMs, allowTestExecutions, maxInflight, capabilities };
        const result = await this.postJson<{ leases?: Lease[] }>(`/work/poll`, body);
        return (result.json && result.json.leases) || [];
    }

    async renew({ runnerId, lease }: RenewRequest): Promise<{ renewed: boolean; lease: Lease | null }> {
        try {
            const result = await this.postJson<{ renewed?: boolean; lease?: Lease }>(`/work/renew`, {
                runnerId,
                lease,
            });
            return {
                renewed: result.json ? !!result.json.renewed : false,
                lease: result.json && result.json.lease ? result.json.lease : null,
            };
        } catch (err) {
            if (err instanceof LeaseNotFoundError) {
                return { renewed: false, lease: null };
            }
            throw err;
        }
    }

    async ack({
        runnerId,
        lease,
        succeeded,
        nextFireTimeUtc,
        deadLetterReason,
    }: AckRequest): Promise<void> {
        const body: AckRequest = { runnerId, lease, succeeded };
        if (nextFireTimeUtc) {
            body.nextFireTimeUtc = nextFireTimeUtc;
        }
        if (deadLetterReason) {
            body.deadLetterReason = deadLetterReason;
        }
        await this.postJson(`/work/ack`, body);
    }

    async events({ runnerId, lease, events }: EventsRequest): Promise<void> {
        const body: EventsRequest = { runnerId, lease, events };
        const path = `/work/${encodeURIComponent(lease.executionId)}:events`;
        await this.postJson(path, body);
    }

    async heartbeat({ runnerId, environmentTag, seenAtUtc, metadataJson }: HeartbeatRequest): Promise<void> {
        const body: HeartbeatRequest = { runnerId };
        if (environmentTag) {
            body.environmentTag = environmentTag;
        }
        if (seenAtUtc) {
            body.seenAtUtc = seenAtUtc;
        }
        if (metadataJson) {
            body.metadataJson = metadataJson;
        }
        await this.postJson(`/runners/heartbeat`, body);
    }

    private buildUrl(path: string): string {
        const url = new URL(
            `${this.baseUrl}/tenants/${encodeURIComponent(this.tenantId)}${path}`
        );
        if (this.environment) {
            url.searchParams.set('environment', this.environment);
        }
        return url.toString();
    }

    private headers(): Record<string, string> {
        const headers: Record<string, string> = { 'Content-Type': 'application/json' };
        if (this.bearerToken) {
            headers.Authorization = `Bearer ${this.bearerToken}`;
            return headers;
        }
        headers['X-Croniq-Key'] = this.apiKey;
        return headers;
    }

    private async postJson<T>(path: string, body: unknown): Promise<PostResult<T>> {
        const response = await this.fetchImpl(this.buildUrl(path), {
            method: 'POST',
            headers: this.headers(),
            body: JSON.stringify(body),
        });

        if (response.status === 204) {
            return { status: 204, json: null };
        }

        let json: T | null = null;
        try {
            json = (await response.json()) as T;
        } catch {
            // ignore json errors
        }

        if (response.status >= 200 && response.status < 300) {
            return { status: response.status, json };
        }

        const bodyText = await response.text();
        if (response.status === 403 && isRunnerMismatchResponse(json, bodyText)) {
            throw new RunnerMismatchError(bodyText);
        }
        if (response.status === 409) {
            throw new LeaseConflictError(bodyText);
        }
        if (response.status === 404) {
            throw new LeaseNotFoundError(bodyText);
        }
        throw new CroniqError('request failed', response.status, bodyText);
    }
}

type GrpcWorkAssigned = {
    execution_id: string;
    lease_id: string;
    trigger_id: string;
    job_key: string;
    fire_at_utc: string | number;
    lease_expires_at_utc: string | number;
    payload: string;
    execution_mode?: string;
    invocation_source?: string;
};

type GrpcRunnerHello = {
    runner_id: string;
    max_inflight?: number;
    capabilities?: Record<string, string>;
    allow_test_executions?: boolean;
};

type GrpcRunnerMessage = {
    hello?: GrpcRunnerHello;
    ack_success?: { execution_id: string; lease_id: string };
    ack_failure?: {
        execution_id: string;
        lease_id: string;
        error_type?: string;
        error_message?: string;
        dead_letter_reason?: string;
        next_fire_time_utc?: number;
    };
    events?: {
        execution_id: string;
        lease_id: string;
        events: Array<{
            message: string;
            level?: string;
            timestamp_utc?: number;
            properties?: Record<string, string>;
            event_type?: string;
        }>;
    };
};

type GrpcServerMessage = {
    hello?: { server_id: string; tenant_id: string; environment_tag: string; server_time_utc: number };
    assigned?: GrpcWorkAssigned;
};

type GrpcRunnerClient = grpc.Client & {
    connect: (metadata?: grpc.Metadata) => grpc.ClientDuplexStream<GrpcRunnerMessage, GrpcServerMessage>;
};

type OutboxEntry = {
    id: string;
    type: 'ack_success' | 'ack_failure' | 'events';
    payload: unknown;
    attempts: number;
    createdAt: string;
};

class OutboxStore {
    private readonly filePath: string;
    private readonly maxEntries: number;
    private readonly maxBytes: number;
    private entries: OutboxEntry[] = [];
    private writeLock: Promise<void> = Promise.resolve();

    constructor(filePath: string, maxEntries: number, maxBytes: number) {
        this.filePath = filePath;
        this.maxEntries = maxEntries;
        this.maxBytes = maxBytes;
    }

    async load(): Promise<void> {
        try {
            const raw = await fs.readFile(this.filePath, 'utf-8');
            const lines = raw.split('\n').filter((line) => line.trim().length > 0);
            this.entries = lines.map((line) => JSON.parse(line) as OutboxEntry);
        } catch {
            this.entries = [];
        }
    }

    list(): OutboxEntry[] {
        return [...this.entries];
    }

    async enqueue(entry: OutboxEntry): Promise<void> {
        this.entries.push(entry);
        await this.compact();
        await this.persist();
    }

    async markFailed(entryId: string): Promise<void> {
        const entry = this.entries.find((item) => item.id === entryId);
        if (entry) {
            entry.attempts += 1;
            await this.persist();
        }
    }

    async remove(entryId: string): Promise<void> {
        this.entries = this.entries.filter((item) => item.id !== entryId);
        await this.persist();
    }

    private async compact(): Promise<void> {
        if (this.entries.length > this.maxEntries) {
            this.entries = this.entries.slice(this.entries.length - this.maxEntries);
        }

        await this.persist();
        try {
            const stat = await fs.stat(this.filePath);
            if (stat.size > this.maxBytes) {
                const overshoot = stat.size - this.maxBytes;
                const dropCount = Math.min(this.entries.length, Math.ceil(overshoot / 200));
                this.entries = this.entries.slice(dropCount);
            }
        } catch {
            // ignore
        }
    }

    private async persist(): Promise<void> {
        this.writeLock = this.writeLock.then(async () => {
            const dir = path.dirname(this.filePath);
            await fs.mkdir(dir, { recursive: true });
            const payload = this.entries.map((item) => JSON.stringify(item)).join('\n');
            await fs.writeFile(this.filePath, payload, 'utf-8');
        });
        await this.writeLock;
    }
}

class GrpcRunnerConnection extends EventEmitter {
    private readonly endpoint: string;
    private readonly metadata: grpc.Metadata;
    private readonly runnerId: string;
    private readonly allowTestExecutions: boolean;
    private readonly maxInflight: number;
    private readonly capabilities: string[] | undefined;
    private readonly retryBaseMs: number;
    private readonly retryMaxMs: number;
    private readonly retryMaxAttempts?: number;
    private client: GrpcRunnerClient | null = null;
    private stream: grpc.ClientDuplexStream<GrpcRunnerMessage, GrpcServerMessage> | null = null;
    private connected = false;
    private stopped = false;

    constructor(options: {
        endpoint: string;
        metadata: grpc.Metadata;
        runnerId: string;
        allowTestExecutions: boolean;
        maxInflight: number;
        capabilities?: string[];
        retryBaseMs: number;
        retryMaxMs: number;
        retryMaxAttempts?: number;
    }) {
        super();
        this.endpoint = options.endpoint;
        this.metadata = options.metadata;
        this.runnerId = options.runnerId;
        this.allowTestExecutions = options.allowTestExecutions;
        this.maxInflight = options.maxInflight;
        this.capabilities = options.capabilities;
        this.retryBaseMs = options.retryBaseMs;
        this.retryMaxMs = options.retryMaxMs;
        this.retryMaxAttempts = options.retryMaxAttempts;
    }

    async start(): Promise<void> {
        this.stopped = false;
        await this.connectLoop();
    }

    stop(): void {
        this.stopped = true;
        this.connected = false;
        if (this.stream) {
            this.stream.cancel();
            this.stream = null;
        }
        if (this.client) {
            this.client.close();
            this.client = null;
        }
    }

    isConnected(): boolean {
        return this.connected;
    }

    send(message: GrpcRunnerMessage): void {
        if (this.stream) {
            this.stream.write(message);
        }
    }

    private async connectLoop(): Promise<void> {
        let attempt = 0;
        while (!this.stopped) {
            try {
                await this.connectOnce();
                attempt = 0;
            } catch (err) {
                if (isGrpcRunnerMismatch(err)) {
                    this.emit('error', err);
                    throw err;
                }
                attempt += 1;
                if (this.retryMaxAttempts && attempt >= this.retryMaxAttempts) {
                    this.emit('error', err);
                    return;
                }
                const delay = this.nextDelay(attempt);
                await new Promise((resolve) => setTimeout(resolve, delay));
            }
        }
    }

    private connectOnce(): Promise<void> {
        const protoPath = path.resolve(__dirname, '../../src/Croniq.Rpc.Client/Protos/runner.proto');
        const packageDefinition = protoLoader.loadSync(protoPath, {
            keepCase: true,
            longs: String,
            enums: String,
            defaults: true,
            oneofs: true,
        });
        const proto = grpc.loadPackageDefinition(packageDefinition) as unknown as {
            croniq: { rpc: { Runner: new (addr: string, creds: grpc.ChannelCredentials) => GrpcRunnerClient } };
        };

        const credentials = this.endpoint.startsWith('https://')
            ? grpc.credentials.createSsl()
            : grpc.credentials.createInsecure();
        this.client = new proto.croniq.rpc.Runner(this.endpoint, credentials);
        this.stream = this.client.connect(this.metadata);

        this.stream.on('data', (message) => {
            if (message?.assigned) {
                this.emit('assigned', message.assigned);
            }
            if (message?.hello && !this.connected) {
                this.connected = true;
                this.emit('connected', message.hello);
            }
        });

        this.stream.on('error', (err) => {
            this.connected = false;
            this.emit('disconnected', err);
        });

        this.stream.on('end', () => {
            this.connected = false;
            this.emit('disconnected');
        });

        const capabilities: Record<string, string> = {};
        if (this.capabilities) {
            for (const entry of this.capabilities) {
                if (entry.trim()) {
                    capabilities[entry.trim()] = 'true';
                }
            }
        }

        this.stream.write({
            hello: {
                runner_id: this.runnerId,
                max_inflight: this.maxInflight,
                allow_test_executions: this.allowTestExecutions,
                capabilities,
            },
        });

        return new Promise((resolve, reject) => {
            let didConnect = false;
            const onConnected = () => {
                didConnect = true;
            };
            const onDisconnected = (err?: unknown) => {
                cleanup();
                if (didConnect) {
                    resolve();
                } else {
                    reject(err ?? new Error('gRPC connection closed'));
                }
            };
            const cleanup = () => {
                this.off('connected', onConnected);
                this.off('disconnected', onDisconnected);
            };

            this.on('connected', onConnected);
            this.on('disconnected', onDisconnected);
        });
    }

    private nextDelay(attempt: number): number {
        const base = Math.min(this.retryMaxMs, this.retryBaseMs * 2 ** Math.max(0, attempt - 1));
        const jitter = base * 0.2 * Math.random();
        return Math.round(base + jitter);
    }
}

export class CroniqRunner {
    private readonly config: RunnerConfig;
    private readonly client: RunnerClient;
    private readonly logger: RunnerLogger;
    private readonly inflight = new Map<string, { lease: Lease; renewTimer?: NodeJS.Timeout }>();
    private readonly queue: Lease[] = [];
    private readonly maxInflight: number;
    private readonly allowTestExecutions: boolean;
    private readonly transportMode: TransportMode;
    private readonly pollWaitMs: number;
    private readonly pollBatchSize: number;
    private readonly renewLeadMs: number;
    private readonly parsePayloadJson: boolean;
    private readonly retryBaseMs: number;
    private readonly retryMaxMs: number;
    private readonly retryMaxAttempts?: number;
    private readonly heartbeatIntervalMs: number;
    private readonly heartbeatMetadata?: Record<string, unknown>;
    private readonly outbox: OutboxStore | null;
    private handler: RunnerExecuteHandler | null = null;
    private running = false;
    private grpcConnection: GrpcRunnerConnection | null = null;
    private fatalError: Error | null = null;
    private fatalReject: ((err: Error) => void) | null = null;

    constructor(config: RunnerConfig) {
        const hasApiKey = !!config.apiKey;
        const hasBearerToken = !!config.bearerToken;
        if (hasApiKey === hasBearerToken) {
            throw new Error('apiKey or bearerToken is required (but not both)');
        }
        if (config.transportMode && !['auto', 'grpc', 'polling'].includes(config.transportMode)) {
            throw new Error('transportMode must be auto, grpc, or polling');
        }
        this.config = config;
        this.transportMode = config.transportMode ?? 'auto';
        this.allowTestExecutions = !!config.allowTestExecutions;
        this.maxInflight = Math.max(1, config.maxInflight ?? 1);
        this.pollWaitMs = config.pollWaitMs ?? 25000;
        this.pollBatchSize = config.pollBatchSize ?? this.maxInflight;
        this.renewLeadMs = config.renewLeadMs ?? 10000;
        this.parsePayloadJson = !!config.parsePayloadJson;
        this.retryBaseMs = config.retryBaseMs ?? 500;
        this.retryMaxMs = config.retryMaxMs ?? 10000;
        this.retryMaxAttempts = config.retryMaxAttempts;
        this.heartbeatIntervalMs = Math.max(0, config.heartbeatIntervalMs ?? 0);
        this.heartbeatMetadata = config.heartbeatMetadata;
        const outboxPath = config.outboxPath ?? path.join(process.cwd(), '.croniq', 'runner-outbox.jsonl');
        this.outbox = new OutboxStore(outboxPath, config.outboxMaxEntries ?? 500, config.outboxMaxBytes ?? 1_000_000);

        this.client = new RunnerClient({
            baseUrl: config.baseUrl,
            tenantId: config.tenantId,
            environment: config.environment,
            apiKey: config.apiKey,
            bearerToken: config.bearerToken,
            fetchImpl: config.fetchImpl,
        });

        this.logger = {
            info: (message, data) => console.log(message, data ?? {}),
            warn: (message, data) => console.warn(message, data ?? {}),
            error: (message, data) => console.error(message, data ?? {}),
        };
        this.fatalError = null;
        this.fatalReject = null;
    }

    onExecute(handler: RunnerExecuteHandler): void {
        this.handler = handler;
    }

    async start(): Promise<void> {
        if (!this.handler) {
            throw new Error('onExecute handler must be registered before start');
        }
        this.running = true;
        this.fatalError = null;
        const tasks: Promise<void>[] = [];

        const fatalPromise = new Promise<void>((_, reject) => {
            this.fatalReject = reject;
        });

        if (this.outbox) {
            await this.outbox.load();
            tasks.push(this.replayOutbox());
        }

        if (this.transportMode !== 'polling') {
            tasks.push(this.startGrpc());
        }

        if (this.transportMode !== 'grpc') {
            tasks.push(this.startPolling());
        }

        if (this.heartbeatIntervalMs > 0) {
            tasks.push(this.startHeartbeat());
        }

        tasks.push(this.processLoop());
        try {
            await Promise.race([Promise.all(tasks), fatalPromise]);
        } finally {
            this.fatalReject = null;
        }
    }

    async stop(): Promise<void> {
        this.running = false;
        if (this.grpcConnection) {
            this.grpcConnection.stop();
            this.grpcConnection = null;
        }
        for (const entry of this.inflight.values()) {
            if (entry.renewTimer) {
                clearTimeout(entry.renewTimer);
            }
        }
        this.inflight.clear();
    }

    private async startGrpc(): Promise<void> {
        const endpoint = this.config.grpcBaseUrl ?? this.config.baseUrl;
        const metadata = new grpc.Metadata();
        if (this.config.bearerToken) {
            metadata.set('Authorization', `Bearer ${this.config.bearerToken}`);
        } else if (this.config.apiKey) {
            metadata.set('X-Croniq-Key', this.config.apiKey);
        }

        const connection = new GrpcRunnerConnection({
            endpoint,
            metadata,
            runnerId: this.config.runnerId,
            allowTestExecutions: this.allowTestExecutions,
            maxInflight: this.maxInflight,
            capabilities: this.config.capabilities,
            retryBaseMs: this.retryBaseMs,
            retryMaxMs: this.retryMaxMs,
            retryMaxAttempts: this.retryMaxAttempts,
        });
        this.grpcConnection = connection;

        connection.on('assigned', (assigned: GrpcWorkAssigned) => {
            const lease = this.toLeaseFromGrpc(assigned);
            this.enqueueLease(lease);
        });

        connection.on('error', (err) => {
            if (this.handleRunnerMismatch(err)) {
                return;
            }
            this.logger.error('gRPC transport failed', { error: String(err) });
        });

        await connection.start();
    }

    private async startPolling(): Promise<void> {
        while (this.running) {
            if (this.transportMode === 'auto' && this.grpcConnection?.isConnected()) {
                await this.sleep(250);
                continue;
            }

            try {
                const leases = await this.client.poll({
                    runnerId: this.config.runnerId,
                    batchSize: this.pollBatchSize,
                    waitForMs: this.pollWaitMs,
                    allowTestExecutions: this.allowTestExecutions,
                    maxInflight: this.maxInflight,
                    capabilities: this.config.capabilities,
                });
                for (const lease of leases) {
                    this.enqueueLease(lease);
                }
            } catch (err) {
                if (this.handleRunnerMismatch(err)) {
                    return;
                }
                this.logger.warn('poll failed', { error: String(err) });
                await this.sleep(this.nextDelay(1));
            }
        }
    }

    private async startHeartbeat(): Promise<void> {
        while (this.running) {
            try {
                if (!this.config.environment) {
                    this.logger.warn('heartbeat skipped; environment is required');
                    await this.sleep(this.heartbeatIntervalMs);
                    continue;
                }

                const metadataJson = JSON.stringify(this.buildHeartbeatMetadata());
                await this.client.heartbeat({
                    runnerId: this.config.runnerId,
                    environmentTag: this.config.environment,
                    metadataJson,
                });
            } catch (err) {
                if (this.handleRunnerMismatch(err)) {
                    return;
                }
                this.logger.warn('heartbeat failed', { error: String(err) });
            }

            await this.sleep(this.heartbeatIntervalMs);
        }
    }

    private async processLoop(): Promise<void> {
        while (this.running) {
            if (this.queue.length === 0 || this.inflight.size >= this.maxInflight) {
                await this.sleep(50);
                continue;
            }

            const lease = this.queue.shift();
            if (!lease) {
                continue;
            }

            this.inflight.set(lease.leaseId, { lease });
            this.scheduleRenew(lease);
            void this.executeLease(lease);
        }
    }

    private async executeLease(lease: Lease): Promise<void> {
        const context: RunnerExecutionContext = {
            executionId: lease.executionId,
            leaseId: lease.leaseId,
            triggerId: lease.triggerId,
            jobKey: lease.jobKey,
            fireAtUtc: lease.fireAtUtc,
            leaseExpiresAtUtc: lease.leaseExpiresAtUtc,
            executionMode: lease.executionMode,
            invocationSource: lease.invocationSource,
            emitEvent: async (event) => {
                await this.sendEvents(lease, [event], true);
            },
        };

        if (!this.allowTestExecutions && lease.executionMode?.toLowerCase() === 'test') {
            await this.rejectTestLease(lease);
            this.completeLease(lease.leaseId);
            return;
        }

        const payload = this.parsePayloadJson ? this.tryParsePayload(lease.payload) : lease.payload ?? null;
        try {
            await this.handler?.(context, payload, this.logger);
            await this.ackSuccess(lease);
        } catch (err) {
            await this.ackFailure(lease, err);
        } finally {
            this.completeLease(lease.leaseId);
        }
    }

    private async ackSuccess(lease: Lease, allowOutbox = true): Promise<void> {
        if (this.grpcConnection?.isConnected()) {
            this.grpcConnection.send({
                ack_success: {
                    execution_id: lease.executionId,
                    lease_id: lease.leaseId,
                },
            });
            return;
        }
        try {
            await this.client.ack({ runnerId: this.config.runnerId, lease, succeeded: true });
        } catch (err) {
            if (this.handleRunnerMismatch(err)) {
                return;
            }
            if (allowOutbox && this.outbox) {
                await this.outbox.enqueue({
                    id: randomUUID(),
                    type: 'ack_success',
                    payload: { lease },
                    attempts: 0,
                    createdAt: new Date().toISOString(),
                });
                return;
            }
            throw err;
        }
    }

    private async ackFailure(lease: Lease, err: unknown, allowOutbox = true): Promise<void> {
        const message = err instanceof Error ? err.message : String(err);
        if (this.grpcConnection?.isConnected()) {
            this.grpcConnection.send({
                ack_failure: {
                    execution_id: lease.executionId,
                    lease_id: lease.leaseId,
                    error_type: 'execution-failed',
                    error_message: message,
                },
            });
            return;
        }
        try {
            await this.client.ack({ runnerId: this.config.runnerId, lease, succeeded: false, deadLetterReason: 'execution-failed' });
        } catch (err) {
            if (allowOutbox && this.outbox) {
                await this.outbox.enqueue({
                    id: randomUUID(),
                    type: 'ack_failure',
                    payload: { lease, errorType: 'execution-failed', errorMessage: message, deadLetterReason: 'execution-failed' },
                    attempts: 0,
                    createdAt: new Date().toISOString(),
                });
                return;
            }
            throw err;
        }
    }

    private async rejectTestLease(lease: Lease, allowOutbox = true): Promise<void> {
        if (this.grpcConnection?.isConnected()) {
            this.grpcConnection.send({
                ack_failure: {
                    execution_id: lease.executionId,
                    lease_id: lease.leaseId,
                    error_type: 'test-not-allowed',
                    error_message: 'test executions are disabled for this runner',
                    dead_letter_reason: 'test-not-allowed',
                },
            });
            return;
        }
        try {
            await this.client.ack({
                runnerId: this.config.runnerId,
                lease,
                succeeded: false,
                deadLetterReason: 'test-not-allowed',
            });
        } catch (err) {
            if (this.handleRunnerMismatch(err)) {
                return;
            }
            if (allowOutbox && this.outbox) {
                await this.outbox.enqueue({
                    id: randomUUID(),
                    type: 'ack_failure',
                    payload: { lease, errorType: 'test-not-allowed', errorMessage: 'test executions are disabled for this runner', deadLetterReason: 'test-not-allowed' },
                    attempts: 0,
                    createdAt: new Date().toISOString(),
                });
                return;
            }
            throw err;
        }
    }

    private async sendEvents(lease: Lease, events: WorkEvent[], allowOutbox = true): Promise<void> {
        if (this.grpcConnection?.isConnected()) {
            this.grpcConnection.send({
                events: {
                    execution_id: lease.executionId,
                    lease_id: lease.leaseId,
                    events: events.map((event) => ({
                        message: event.message,
                        level: event.level,
                        timestamp_utc: event.timestampUtc ? Date.parse(event.timestampUtc) : undefined,
                        properties: event.properties,
                        event_type: event.eventType,
                    })),
                },
            });
            return;
        }
        try {
            await this.client.events({ runnerId: this.config.runnerId, lease, events });
        } catch (err) {
            if (this.handleRunnerMismatch(err)) {
                return;
            }
            if (allowOutbox && this.outbox) {
                await this.outbox.enqueue({
                    id: randomUUID(),
                    type: 'events',
                    payload: { lease, events },
                    attempts: 0,
                    createdAt: new Date().toISOString(),
                });
                return;
            }
            throw err;
        }
    }

    private async replayOutbox(): Promise<void> {
        while (this.running && this.outbox) {
            const entries = this.outbox.list();
            if (entries.length === 0) {
                await this.sleep(1000);
                continue;
            }
            for (const entry of entries) {
                try {
                    if (entry.type === 'ack_success') {
                        const lease = (entry.payload as { lease: Lease }).lease;
                        await this.ackSuccess(lease, false);
                    } else if (entry.type === 'ack_failure') {
                        const payload = entry.payload as { lease: Lease; errorType: string; errorMessage: string; deadLetterReason: string };
                        await this.ackFailure(payload.lease, payload.errorMessage, false);
                    } else if (entry.type === 'events') {
                        const payload = entry.payload as { lease: Lease; events: WorkEvent[] };
                        await this.sendEvents(payload.lease, payload.events, false);
                    }
                    await this.outbox.remove(entry.id);
                } catch (err) {
                    if (this.handleRunnerMismatch(err)) {
                        return;
                    }
                    await this.outbox.markFailed(entry.id);
                    await this.sleep(this.nextDelay(entry.attempts + 1));
                }
            }
        }
    }

    private enqueueLease(lease: Lease): void {
        this.queue.push(lease);
    }

    private scheduleRenew(lease: Lease): void {
        const entry = this.inflight.get(lease.leaseId);
        if (!entry) {
            return;
        }

        if (entry.renewTimer) {
            clearTimeout(entry.renewTimer);
        }

        const expiresAt = Date.parse(lease.leaseExpiresAtUtc);
        const delay = Math.max(1000, expiresAt - Date.now() - this.renewLeadMs);
        entry.renewTimer = setTimeout(async () => {
            if (!this.inflight.has(lease.leaseId)) {
                return;
            }
            try {
                const { renewed, lease: updated } = await this.client.renew({
                    runnerId: this.config.runnerId,
                    lease,
                });
                if (renewed && updated) {
                    entry.lease = updated;
                    this.scheduleRenew(updated);
                }
            } catch (err) {
                if (this.handleRunnerMismatch(err)) {
                    return;
                }
                this.logger.warn('renew failed', { error: String(err), leaseId: lease.leaseId });
            }
        }, delay);
    }

    private completeLease(leaseId: string): void {
        const entry = this.inflight.get(leaseId);
        if (entry?.renewTimer) {
            clearTimeout(entry.renewTimer);
        }
        this.inflight.delete(leaseId);
    }

    private toLeaseFromGrpc(assigned: GrpcWorkAssigned): Lease {
        const fireAt = this.toIsoString(assigned.fire_at_utc);
        const expiresAt = this.toIsoString(assigned.lease_expires_at_utc);
        return {
            executionId: assigned.execution_id,
            leaseId: assigned.lease_id,
            triggerId: assigned.trigger_id,
            jobKey: assigned.job_key,
            fireAtUtc: fireAt,
            leaseExpiresAtUtc: expiresAt,
            payload: assigned.payload || null,
            executionMode: assigned.execution_mode,
            invocationSource: assigned.invocation_source,
        };
    }

    private toIsoString(value: string | number): string {
        const parsed = typeof value === 'string' ? Number.parseInt(value, 10) : value;
        return new Date(parsed).toISOString();
    }

    private tryParsePayload(payload?: string | null): unknown {
        if (!payload) {
            return null;
        }
        try {
            return JSON.parse(payload);
        } catch {
            return payload;
        }
    }

    private buildHeartbeatMetadata(): Record<string, unknown> {
        const transportState = this.grpcConnection?.isConnected() ? 'grpc' : 'polling';
        return {
            transportMode: this.transportMode,
            transportState,
            allowTestExecutions: this.allowTestExecutions,
            maxInflight: this.maxInflight,
            capabilities: this.config.capabilities ?? [],
            ...this.heartbeatMetadata,
        };
    }

    private handleRunnerMismatch(err: unknown): boolean {
        if (err instanceof RunnerMismatchError) {
            this.logger.error('runner mismatch', { error: err.body });
            this.failFatal(err);
            return true;
        }
        return false;
    }

    private failFatal(err: Error): void {
        if (this.fatalError) {
            return;
        }
        this.fatalError = err;
        this.running = false;
        if (this.grpcConnection) {
            this.grpcConnection.stop();
            this.grpcConnection = null;
        }
        if (this.fatalReject) {
            this.fatalReject(err);
        }
    }

    private async sleep(ms: number): Promise<void> {
        await new Promise((resolve) => setTimeout(resolve, ms));
    }

    private nextDelay(attempt: number): number {
        const base = Math.min(this.retryMaxMs, this.retryBaseMs * 2 ** Math.max(0, attempt - 1));
        const jitter = base * 0.2 * Math.random();
        return Math.round(base + jitter);
    }
}
