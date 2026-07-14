import { existsSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { CroniqTriggerClient, createTriggerClient, type TriggerParams } from '@nuetzliches/croniq-runner';
import { describe, expect, it } from 'vitest';

import { matchBody } from './body-matcher.js';
import { loadTriggerCase } from './case-loader.js';
import { MockServer, type RecordedRequest } from './mock-server.js';
import type {
  TriggerCall,
  TriggerCaseSpec,
  TriggerHttpExpectation,
  TriggerResponseExpect,
} from './trigger-case-spec.js';

const __dirname = dirname(fileURLToPath(import.meta.url));
const CASES_DIR = resolve(__dirname, '../../../cases-trigger');

const caseFiles = existsSync(CASES_DIR)
  ? readdirSync(CASES_DIR)
      .filter((f) => f.endsWith('.yaml'))
      .sort()
  : [];

describe('Croniq trigger (producer) conformance (TypeScript binding)', () => {
  // The runner is wired unconditionally; the shared producer cases land with
  // #287 (sdks/conformance/cases-trigger/). Until then there is nothing to
  // drive — skip explicitly so the intent stays visible in test output.
  if (caseFiles.length === 0) {
    it.skip('no trigger cases present (sdks/conformance/cases-trigger absent — lands with #287)', () => {
      /* intentionally empty */
    });
    return;
  }

  it.each(caseFiles)('%s', async (file) => {
    const spec = loadTriggerCase(join(CASES_DIR, file));
    await runTriggerCase(spec);
  });
});

async function runTriggerCase(spec: TriggerCaseSpec): Promise<void> {
  const mock = new MockServer(spec.server_script);
  const baseUrl = await mock.start();
  try {
    const client = createTriggerClient({
      serverUrl: baseUrl,
      apiKey: spec.trigger_config.api_key,
      bearerToken: spec.trigger_config.bearer_token,
    });

    // trigger_calls are ordered and per-call: make each in sequence and
    // assert its own outcome. Multi-call cases (dedup) rely on the mock
    // sequencing responses via match_count.
    for (const [index, call] of spec.trigger_calls.entries()) {
      await runTriggerCall(client, call, index);
    }

    assertHttp(spec, mock.recorded);
  } finally {
    await mock.stop();
  }
}

async function runTriggerCall(client: CroniqTriggerClient, call: TriggerCall, index: number): Promise<void> {
  const req = call.request;
  const params: TriggerParams = {};
  if (req.metadata !== undefined) params.metadata = req.metadata;
  if (req.require !== undefined) params.require = req.require;
  if (req.prefer !== undefined) params.prefer = req.prefer;
  if (req.timeout !== undefined) params.timeout = req.timeout;
  if (req.idempotency_key !== undefined) params.idempotencyKey = req.idempotency_key;

  const label = `trigger_calls[${index}] (${req.job_key})`;

  if (call.expect.error) {
    let threw = false;
    try {
      await client.trigger(req.job_key, params);
    } catch {
      threw = true;
    }
    expect(threw, `${label}: expected the call to surface an error`).toBe(true);
    return;
  }

  const result = await client.trigger(req.job_key, params);
  assertResponse(call.expect.response, result, label);
}

function assertResponse(
  expected: TriggerResponseExpect | undefined,
  result: { executionId: string; queued: number; deduplicated: boolean },
  label: string,
): void {
  if (!expected) return;
  if (expected.execution_id !== undefined) {
    if (expected.execution_id === '*') {
      expect(result.executionId, `${label}: expected non-empty execution_id`).toBeTruthy();
    } else {
      expect(result.executionId, `${label}: execution_id`).toBe(expected.execution_id);
    }
  }
  if (expected.queued !== undefined) {
    expect(result.queued, `${label}: queued`).toBe(expected.queued);
  }
  if (expected.deduplicated !== undefined) {
    expect(result.deduplicated, `${label}: deduplicated`).toBe(expected.deduplicated);
  }
}

function assertHttp(spec: TriggerCaseSpec, recorded: RecordedRequest[]): void {
  for (const ex of spec.expectations.http) {
    const matches = recorded.filter(
      (r) => r.method.toUpperCase() === ex.method.toUpperCase() && r.path === ex.path,
    );
    const label = `${ex.method} ${ex.path}`;

    if (typeof ex.exact_count === 'number') {
      expect(matches.length, `${label}: expected exact_count=${ex.exact_count}`).toBe(ex.exact_count);
    }
    if (typeof ex.min_count === 'number') {
      expect(matches.length, `${label}: expected min_count=${ex.min_count}`).toBeGreaterThanOrEqual(ex.min_count);
    }
    if (typeof ex.max_count === 'number') {
      expect(matches.length, `${label}: expected max_count=${ex.max_count}`).toBeLessThanOrEqual(ex.max_count);
    }

    if (ex.headers && matches.length > 0) {
      const first = matches[0]!;
      for (const [name, expectedValue] of Object.entries(ex.headers)) {
        const actual = first.headers[name.toLowerCase()];
        expect(actual, `${label}: missing header '${name}'`).toBeDefined();
        if (expectedValue === '*') {
          expect(actual, `${label}: header '${name}' was empty`).toBeTruthy();
        } else {
          expect(actual, `${label}: header '${name}' mismatch`).toBe(expectedValue);
        }
      }
    }

    if (ex.body_match !== undefined && matches.length > 0) {
      const parsed = parseBody(matches[0]!);
      const err = matchBody(ex.body_match, parsed);
      expect(err, `${label}: body mismatch — ${err ?? ''}`).toBeNull();
    }

    if (ex.body_absent && matches.length > 0) {
      const parsed = parseBody(matches[0]!) as Record<string, unknown> | null;
      for (const key of ex.body_absent) {
        const present = parsed !== null && typeof parsed === 'object' && key in parsed;
        expect(present, `${label}: body key '${key}' must be absent`).toBe(false);
      }
    }
  }
}

function parseBody(request: RecordedRequest): unknown {
  return request.body.length > 0 ? JSON.parse(request.body) : null;
}
