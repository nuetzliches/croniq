import type { Logger } from './logger.js';
import { assertSecureServerUrl } from './security.js';
import { trimTrailingSlashes } from './url.js';

export interface LogWriterOptions {
  /** Bounded queue capacity. Backpressure kicks in when full. Default 256. */
  channelCapacity?: number;
  /** Flush when this many events have accumulated. Default 32. */
  batchSizeThreshold?: number;
  /** Flush at least this often, even if the size threshold isn't reached. Default 200 ms. */
  batchTimeThresholdMs?: number;
  /** Maximum events per outgoing HTTP POST. Default 100. */
  maxBatchPerPost?: number;
  /** Maximum time the runner waits for queued events to flush during per-execution drain. Default 5 s. */
  shutdownTimeoutMs?: number;
}

export interface ResolvedLogWriterOptions {
  channelCapacity: number;
  batchSizeThreshold: number;
  batchTimeThresholdMs: number;
  maxBatchPerPost: number;
  shutdownTimeoutMs: number;
}

export interface CroniqRunnerOptions {
  /**
   * Base URL of the Croniq server, e.g. `http://localhost:4000`.
   *
   * `https://` is required unless the host is loopback (`localhost`,
   * `127.0.0.0/8`, `::1`) — the API key rides along on every request and would
   * otherwise travel in cleartext. See {@link allowInsecureHttp}.
   */
  serverUrl: string;

  /**
   * Opt in to a cleartext `http://` {@link serverUrl} on a non-loopback host.
   *
   * Off by default: without it such a URL is rejected by `resolveOptions`.
   * With it, the SDK still emits one loud startup warning — the API key then
   * travels in cleartext on every poll. Default `false`.
   */
  allowInsecureHttp?: boolean;

  /**
   * Stable runner identifier. If omitted, the SDK resolves it via
   * `RUNNER_ID` env var → state file under {@link runnerDataDir} →
   * a newly generated `{prefix}-{hex8}` persisted to the state file.
   */
  runnerId?: string;

  /** Prefix used when generating a fresh runner ID. Default `"runner"`. */
  runnerIdPrefix?: string;

  /**
   * Directory the SDK reads/writes the persistent runner-id file in.
   * Honors `CRONIQ_RUNNER_DATA_DIR` when omitted. Defaults to
   * `$XDG_STATE_HOME/croniq-runner` (or platform equivalent).
   */
  runnerDataDir?: string;

  /** API key used for `Authorization: ApiKey {key}`. Takes precedence over {@link bearerToken}. */
  apiKey?: string;

  /** Bearer token used for `Authorization: Bearer {token}`. */
  bearerToken?: string;

  /**
   * Capabilities the runner advertises (e.g. `"billing"`, `"reporting"`).
   * Used by the server for job routing (`require` / `prefer` in the Croniqfile).
   */
  capabilities?: string[];

  /**
   * Free-form tags the runner declares about itself. Filter-only — does
   * NOT influence routing (capabilities do that). Convention is
   * `key=value` strings (e.g. `"env=prod"`, `"lang=typescript"`).
   */
  tags?: string[];

  /** Maximum concurrent in-flight executions. Default 5, range [1, 1024]. */
  maxInflight?: number;

  /** Per-request timeout for the long-poll work endpoint. Default 35 000 ms. */
  pollTimeoutMs?: number;

  /** Interval at which the runner sends lease-renewal heartbeats. Default 15 000 ms. */
  renewIntervalMs?: number;

  /** Maximum time the runner waits for in-flight handlers during graceful shutdown. Default 30 000 ms. */
  drainTimeoutMs?: number;

  /** Backoff after a failed poll request. Default 5 000 ms. */
  pollRetryDelayMs?: number;

  /** Idle delay when the runner is at `maxInflight` capacity. Default 500 ms. */
  capacityBackoffMs?: number;

  /**
   * How many consecutive `409 Conflict` responses from `POST /v1/work/poll`
   * the runner tolerates before `run()` rejects with a
   * `PollInstanceConflictError`. Default 3, range [1, 100].
   *
   * A sustained 409 means a second process is registered under the same
   * `runnerId` and no amount of retrying fixes that. The counter resets on a
   * successful poll or on any non-409 failure (5xx, network, timeout), which
   * say nothing about instance ownership.
   */
  maxConsecutivePollConflicts?: number;

  /** Streaming log-writer tunables. */
  logWriter?: LogWriterOptions;

  /** Optional logger for SDK-level diagnostics. Defaults to a console logger that emits warn/error only. */
  logger?: Logger;
}

export interface ResolvedRunnerOptions {
  serverUrl: string;
  allowInsecureHttp: boolean;
  runnerId: string | undefined;
  runnerIdPrefix: string;
  runnerDataDir: string | undefined;
  apiKey: string | undefined;
  bearerToken: string | undefined;
  capabilities: string[];
  tags: string[];
  maxInflight: number;
  pollTimeoutMs: number;
  renewIntervalMs: number;
  drainTimeoutMs: number;
  pollRetryDelayMs: number;
  capacityBackoffMs: number;
  maxConsecutivePollConflicts: number;
  logWriter: ResolvedLogWriterOptions;
  logger: Logger;
}

const DEFAULT_LOG_WRITER: ResolvedLogWriterOptions = {
  channelCapacity: 256,
  batchSizeThreshold: 32,
  batchTimeThresholdMs: 200,
  maxBatchPerPost: 100,
  shutdownTimeoutMs: 5_000,
};

export function resolveOptions(input: CroniqRunnerOptions, defaultLogger: Logger): ResolvedRunnerOptions {
  if (!input.serverUrl || typeof input.serverUrl !== 'string') {
    throw new TypeError('CroniqRunnerOptions.serverUrl is required');
  }
  try {
    // eslint-disable-next-line no-new
    new URL(input.serverUrl);
  } catch {
    throw new TypeError(`CroniqRunnerOptions.serverUrl is not a valid URL: ${input.serverUrl}`);
  }

  const logger = input.logger ?? defaultLogger;

  // Fail fast on a cleartext base URL that would leak the API key (#440).
  const allowInsecureHttp = input.allowInsecureHttp ?? false;
  assertSecureServerUrl(input.serverUrl, allowInsecureHttp, 'CroniqRunnerOptions.serverUrl', logger);

  const maxInflight = input.maxInflight ?? 5;
  if (!Number.isInteger(maxInflight) || maxInflight < 1 || maxInflight > 1024) {
    throw new RangeError(`CroniqRunnerOptions.maxInflight must be an integer in [1, 1024], got ${maxInflight}`);
  }

  const maxConsecutivePollConflicts = input.maxConsecutivePollConflicts ?? 3;
  if (
    !Number.isInteger(maxConsecutivePollConflicts) ||
    maxConsecutivePollConflicts < 1 ||
    maxConsecutivePollConflicts > 100
  ) {
    throw new RangeError(
      `CroniqRunnerOptions.maxConsecutivePollConflicts must be an integer in [1, 100], got ${maxConsecutivePollConflicts}`,
    );
  }

  const logWriter: ResolvedLogWriterOptions = {
    channelCapacity: input.logWriter?.channelCapacity ?? DEFAULT_LOG_WRITER.channelCapacity,
    batchSizeThreshold: input.logWriter?.batchSizeThreshold ?? DEFAULT_LOG_WRITER.batchSizeThreshold,
    batchTimeThresholdMs: input.logWriter?.batchTimeThresholdMs ?? DEFAULT_LOG_WRITER.batchTimeThresholdMs,
    maxBatchPerPost: input.logWriter?.maxBatchPerPost ?? DEFAULT_LOG_WRITER.maxBatchPerPost,
    shutdownTimeoutMs: input.logWriter?.shutdownTimeoutMs ?? DEFAULT_LOG_WRITER.shutdownTimeoutMs,
  };

  return {
    serverUrl: trimTrailingSlashes(input.serverUrl),
    allowInsecureHttp,
    runnerId: input.runnerId,
    runnerIdPrefix: input.runnerIdPrefix ?? 'runner',
    runnerDataDir: input.runnerDataDir,
    apiKey: input.apiKey,
    bearerToken: input.bearerToken,
    capabilities: [...(input.capabilities ?? [])],
    tags: [...(input.tags ?? [])],
    maxInflight,
    pollTimeoutMs: input.pollTimeoutMs ?? 35_000,
    renewIntervalMs: input.renewIntervalMs ?? 15_000,
    drainTimeoutMs: input.drainTimeoutMs ?? 30_000,
    pollRetryDelayMs: input.pollRetryDelayMs ?? 5_000,
    capacityBackoffMs: input.capacityBackoffMs ?? 500,
    maxConsecutivePollConflicts,
    logWriter,
    logger,
  };
}
