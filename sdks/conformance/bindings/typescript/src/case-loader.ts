import { readFileSync } from 'node:fs';

import { load } from 'js-yaml';

import {
  assertKnownKeys,
  CASE_KEYS,
  EXPECTATIONS_KEYS,
  HANDLER_KEYS,
  HTTP_EXPECTATION_KEYS,
  RESPOND_KEYS,
  RUNNER_CONFIG_KEYS,
  SCRIPT_ENTRY_KEYS,
  TRIGGER_CALL_KEYS,
  TRIGGER_CASE_KEYS,
  TRIGGER_CONFIG_KEYS,
  TRIGGER_EXPECT_KEYS,
  TRIGGER_HTTP_EXPECTATION_KEYS,
  TRIGGER_REQUEST_KEYS,
  TRIGGER_RESPONSE_KEYS,
} from './case-keys.js';
import type { CaseSpec } from './case-spec.js';
import type { TriggerCaseSpec } from './trigger-case-spec.js';

export function loadCase(path: string): CaseSpec {
  const text = readFileSync(path, 'utf8');
  const parsed = load(text) as CaseSpec;
  if (!parsed || typeof parsed !== 'object') {
    throw new Error(`failed to parse conformance case at ${path}`);
  }
  validateCase(parsed, path);
  return parsed;
}

export function loadTriggerCase(path: string): TriggerCaseSpec {
  const text = readFileSync(path, 'utf8');
  const parsed = load(text) as TriggerCaseSpec;
  if (!parsed || typeof parsed !== 'object') {
    throw new Error(`failed to parse trigger conformance case at ${path}`);
  }
  validateTriggerCase(parsed, path);
  return parsed;
}

/**
 * Reject any key `CaseSpec` does not model.
 *
 * `load(text) as CaseSpec` is a compile-time assertion and nothing more — at
 * runtime an unrecognised key simply sits in the object and is never read, so a
 * case carrying an assertion this binding has not implemented would pass by
 * doing nothing (#460). The walk below is what makes that a failure.
 */
function validateCase(spec: CaseSpec, path: string): void {
  const where = (ctx: string) => `${path}: ${ctx}`;

  assertKnownKeys(spec, CASE_KEYS, where('case'));
  assertKnownKeys(spec.runner_config, RUNNER_CONFIG_KEYS, where('runner_config'));

  for (const handler of spec.handlers ?? []) {
    assertKnownKeys(handler, HANDLER_KEYS, where(`handler '${handler?.job_key ?? '?'}'`));
  }

  validateServerScript(spec.server_script, where);

  assertKnownKeys(spec.expectations, EXPECTATIONS_KEYS, where('expectations'));
  for (const expectation of spec.expectations?.http ?? []) {
    assertKnownKeys(expectation, HTTP_EXPECTATION_KEYS, where(httpCtx(expectation)));
  }
}

/** Trigger-side mirror of {@link validateCase}. */
function validateTriggerCase(spec: TriggerCaseSpec, path: string): void {
  const where = (ctx: string) => `${path}: ${ctx}`;

  assertKnownKeys(spec, TRIGGER_CASE_KEYS, where('trigger case'));
  assertKnownKeys(spec.trigger_config, TRIGGER_CONFIG_KEYS, where('trigger_config'));

  for (const call of spec.trigger_calls ?? []) {
    const ctx = `trigger_calls request '${call?.request?.job_key ?? '?'}'`;
    assertKnownKeys(call, TRIGGER_CALL_KEYS, where('trigger_calls entry'));
    assertKnownKeys(call?.request, TRIGGER_REQUEST_KEYS, where(ctx));
    assertKnownKeys(call?.expect, TRIGGER_EXPECT_KEYS, where(`expect of ${ctx}`));
    assertKnownKeys(
      call?.expect?.response,
      TRIGGER_RESPONSE_KEYS,
      where(`expect.response of ${ctx}`),
    );
  }

  validateServerScript(spec.server_script, where);

  assertKnownKeys(spec.expectations, EXPECTATIONS_KEYS, where('expectations'));
  for (const expectation of spec.expectations?.http ?? []) {
    assertKnownKeys(expectation, TRIGGER_HTTP_EXPECTATION_KEYS, where(httpCtx(expectation)));
  }
}

/** Shared by both case shapes — the mock-server contract is identical. */
function validateServerScript(
  script: readonly { on?: string; respond?: unknown }[] | undefined,
  where: (ctx: string) => string,
): void {
  for (const entry of script ?? []) {
    const label = `'${entry?.on ?? '?'}'`;
    assertKnownKeys(entry, SCRIPT_ENTRY_KEYS, where(`server_script entry ${label}`));
    assertKnownKeys(entry?.respond, RESPOND_KEYS, where(`respond of ${label}`));
  }
}

function httpCtx(expectation: { method?: string; path?: string } | undefined): string {
  return `http expectation ${expectation?.method ?? '?'} ${expectation?.path ?? '?'}`;
}

/** Split `"METHOD /path"` into (method, path). */
export function splitOn(on: string): { method: string; path: string } {
  const idx = on.indexOf(' ');
  if (idx < 0) throw new Error(`invalid server_script.on rule: ${on}`);
  return { method: on.slice(0, idx).toUpperCase(), path: on.slice(idx + 1) };
}
