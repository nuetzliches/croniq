import { readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { CroniqRunner, type CroniqRunnerOptions } from '@nuetzliches/croniq-runner';
import { describe, expect, it } from 'vitest';

import { matchBody } from './body-matcher.js';
import { loadCase } from './case-loader.js';
import type { CaseSpec, RunnerConfig } from './case-spec.js';
import { applyHandlers } from './handler-sentinels.js';
import { MockServer, type RecordedRequest } from './mock-server.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const CASES_DIR = resolve(__dirname, '../../../cases');

const caseFiles = readdirSync(CASES_DIR)
  .filter((f) => f.endsWith('.yaml'))
  .sort();

describe('Croniq runner conformance (TypeScript binding)', () => {
  it.each(caseFiles)('%s', async (file) => {
    const spec = loadCase(join(CASES_DIR, file));
    await runCase(spec);
  });
});

async function runCase(spec: CaseSpec): Promise<void> {
  const mock = new MockServer(spec.server_script);
  const baseUrl = await mock.start();
  try {
    const runner = new CroniqRunner(buildOptions(spec.runner_config, baseUrl));
    applyHandlers(runner, spec.handlers);

    const runAC = new AbortController();
    const deadlineMs = spec.expectations.duration_max_ms ?? 5_000;
    const deadlineTimer = setTimeout(() => runAC.abort(), deadlineMs);

    let shutdownTimer: NodeJS.Timeout | undefined;
    if (typeof spec.shutdown_after_ms === 'number') {
      shutdownTimer = setTimeout(() => runner.requestDrain(), spec.shutdown_after_ms);
    }

    const runPromise = runner.run(runAC.signal).catch((err) => {
      if ((err as Error)?.name === 'AbortError') return;
      throw err;
    });

    const hasMaxCount = spec.expectations.http.some((e) => typeof e.max_count === 'number');
    const pollDeadline = Date.now() + deadlineMs;
    while (Date.now() < pollDeadline) {
      if (!hasMaxCount && expectationsAreMet(spec, mock.recorded)) {
        break;
      }
      await sleep(50);
    }

    runner.requestDrain();
    runAC.abort();
    if (shutdownTimer) clearTimeout(shutdownTimer);
    clearTimeout(deadlineTimer);
    await runPromise;

    if (process.env.CRONIQ_CONFORMANCE_DEBUG === '1') {
      // eslint-disable-next-line no-console
      console.error(`[debug-${spec.name}] ${mock.recorded.length} request(s):`);
      for (const r of mock.recorded) {
        // eslint-disable-next-line no-console
        console.error(`[debug]   ${r.method} ${r.path}`);
      }
    }

    assertExpectations(spec, mock.recorded);
  } finally {
    await mock.stop();
  }
}

function buildOptions(cfg: RunnerConfig, serverUrl: string): CroniqRunnerOptions {
  const opts: CroniqRunnerOptions = { serverUrl };
  if (cfg.runner_id !== undefined) opts.runnerId = cfg.runner_id;
  if (cfg.runner_id_prefix !== undefined) opts.runnerIdPrefix = cfg.runner_id_prefix;
  if (cfg.capabilities !== undefined) opts.capabilities = cfg.capabilities;
  if (cfg.tags !== undefined) opts.tags = cfg.tags;
  if (cfg.max_inflight !== undefined) opts.maxInflight = cfg.max_inflight;
  if (cfg.api_key !== undefined) opts.apiKey = cfg.api_key;
  if (cfg.bearer_token !== undefined) opts.bearerToken = cfg.bearer_token;
  if (cfg.poll_timeout_ms !== undefined) opts.pollTimeoutMs = cfg.poll_timeout_ms;
  if (cfg.renew_interval_ms !== undefined) opts.renewIntervalMs = cfg.renew_interval_ms;
  if (cfg.drain_timeout_ms !== undefined) opts.drainTimeoutMs = cfg.drain_timeout_ms;
  if (cfg.poll_retry_delay_ms !== undefined) opts.pollRetryDelayMs = cfg.poll_retry_delay_ms;
  if (cfg.capacity_backoff_ms !== undefined) opts.capacityBackoffMs = cfg.capacity_backoff_ms;

  // Conformance cases run fast: tighten the log writer's time-threshold so a
  // log-streaming case doesn't time out waiting on a 200 ms tick.
  opts.logWriter = { batchTimeThresholdMs: 50, shutdownTimeoutMs: 3_000 };
  return opts;
}

function expectationsAreMet(spec: CaseSpec, recorded: RecordedRequest[]): boolean {
  for (const ex of spec.expectations.http) {
    const matches = recorded.filter(
      (r) => r.method.toUpperCase() === ex.method.toUpperCase() && r.path === ex.path,
    );
    if (typeof ex.exact_count === 'number' && matches.length < ex.exact_count) return false;
    if (typeof ex.min_count === 'number' && matches.length < ex.min_count) return false;
  }
  return true;
}

function assertExpectations(spec: CaseSpec, recorded: RecordedRequest[]): void {
  for (const ex of spec.expectations.http) {
    const matches = recorded.filter(
      (r) => r.method.toUpperCase() === ex.method.toUpperCase() && r.path === ex.path,
    );
    const label = `${ex.method} ${ex.path}`;

    if (typeof ex.exact_count === 'number') {
      expect(matches.length, `${label}: expected exact_count=${ex.exact_count}`).toBe(ex.exact_count);
    }
    if (typeof ex.min_count === 'number') {
      expect(matches.length, `${label}: expected min_count=${ex.min_count}`).toBeGreaterThanOrEqual(
        ex.min_count,
      );
    }
    if (typeof ex.max_count === 'number') {
      expect(matches.length, `${label}: expected max_count=${ex.max_count}`).toBeLessThanOrEqual(
        ex.max_count,
      );
    }

    if (ex.headers && matches.length > 0) {
      const first = matches[0]!;
      for (const [name, expected] of Object.entries(ex.headers)) {
        const lower = name.toLowerCase();
        const actual = first.headers[lower];
        expect(actual, `${label}: missing header '${name}'`).toBeDefined();
        if (expected === '*') {
          expect(actual, `${label}: header '${name}' was empty`).toBeTruthy();
        } else {
          expect(actual, `${label}: header '${name}' mismatch`).toBe(expected);
        }
      }
    }

    if (ex.body_match !== undefined && matches.length > 0) {
      const first = matches[0]!;
      const parsed = first.body.length > 0 ? JSON.parse(first.body) : null;
      const err = matchBody(ex.body_match, parsed);
      expect(err, `${label}: body mismatch — ${err ?? ''}`).toBeNull();
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
