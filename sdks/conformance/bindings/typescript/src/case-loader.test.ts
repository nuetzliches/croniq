import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

import { loadCase, loadTriggerCase } from './case-loader.js';

// `load(text) as CaseSpec` was a compile-time assertion and nothing more: an
// unrecognised key sat in the object and was never read, so a case carrying an
// assertion this binding had not implemented passed by doing nothing (#460).
// These tests provoke that silence and assert it is now noisy — without them,
// dropping the validation back out would be an invisible regression.

const MINIMAL_CASE = `
name: strictness probe
runner_config:
  capabilities: ["work"]
handlers:
  - job_key: "work:probe"
    behavior: noop
server_script:
  - on: "POST /v1/work/poll"
    respond:
      status: 200
      body: { work: [], cancel: [] }
expectations:
  duration_max_ms: 500
  http:
    - method: POST
      path: /v1/work/poll
      min_count: 1
`;

const MINIMAL_TRIGGER_CASE = `
name: strictness probe
trigger_config:
  api_key: "croniq_testkey"
trigger_calls:
  - request:
      job_key: "work:probe"
    expect:
      response:
        execution_id: "*"
server_script:
  - on: "POST /v1/trigger"
    respond:
      status: 200
      body: { execution_id: "exec-001", queued: 1, deduplicated: false }
expectations:
  duration_max_ms: 500
  http:
    - method: POST
      path: /v1/trigger
      exact_count: 1
`;

function writeTemp(text: string): string {
  const path = join(mkdtempSync(join(tmpdir(), 'croniq-case-')), 'case.yaml');
  writeFileSync(path, text, 'utf8');
  return path;
}

/**
 * Insert an unrecognised key after `anchor` at column `indent`.
 *
 * The indent selects *which* mapping gains the key, so it cannot always be read
 * off the anchor line: a key of a `- ` list item sits two columns right of the
 * dash, and closing a nested block means dedenting below the anchor.
 */
function inject(text: string, anchor: string, indent?: number): string {
  expect(text).toContain(anchor);
  const column = indent ?? anchor.length - anchor.trimStart().length;
  return text.replace(anchor, `${anchor}\n${' '.repeat(column)}not_a_real_key: 1`);
}

describe('loadCase', () => {
  // One entry per level a runner case nests — an unknown key must be caught at
  // each of them, not merely at the top.
  const levels: [name: string, anchor: string, indent?: number][] = [
    ['case', 'name: strictness probe'],
    ['runner_config', '  capabilities: ["work"]'],
    ['handler', '    behavior: noop'],
    ['server_script entry', '  - on: "POST /v1/work/poll"', 4],
    ['respond', '      status: 200'],
    ['expectations', '  duration_max_ms: 500'],
    ['http expectation', '      min_count: 1'],
  ];

  for (const [level, anchor, indent] of levels) {
    it(`rejects a key the binding does not model in ${level}`, () => {
      const path = writeTemp(inject(MINIMAL_CASE, anchor, indent));
      expect(() => loadCase(path)).toThrow(/not_a_real_key/);
    });
  }

  it('rejects body_absent, which only trigger cases may declare', () => {
    const path = writeTemp(
      MINIMAL_CASE.replace('      min_count: 1', '      min_count: 1\n      body_absent: [metadata]'),
    );
    expect(() => loadCase(path)).toThrow(/body_absent/);
  });

  // Counterweight: strictness must not reject what the corpus legitimately
  // uses. Also keeps the fixtures honest — one that failed to load on its own
  // would make every negative test above pass for the wrong reason.
  it('accepts the known vocabulary', () => {
    const spec = loadCase(writeTemp(MINIMAL_CASE));
    expect(spec.name).toBe('strictness probe');
    expect(spec.handlers).toHaveLength(1);
  });
});

describe('loadTriggerCase', () => {
  const levels: [name: string, anchor: string, indent?: number][] = [
    ['trigger case', 'name: strictness probe'],
    ['trigger_config', '  api_key: "croniq_testkey"'],
    ['request', '      job_key: "work:probe"'],
    // Same anchor, two indents: dedenting to 6 closes `response:` and adds the
    // key to `expect`; staying at 8 adds it to `response` itself.
    ['expect', '        execution_id: "*"', 6],
    ['expect.response', '        execution_id: "*"'],
    ['http expectation', '      exact_count: 1'],
  ];

  for (const [level, anchor, indent] of levels) {
    it(`rejects a key the binding does not model in ${level}`, () => {
      const path = writeTemp(inject(MINIMAL_TRIGGER_CASE, anchor, indent));
      expect(() => loadTriggerCase(path)).toThrow(/not_a_real_key/);
    });
  }

  it('accepts body_absent', () => {
    const path = writeTemp(
      MINIMAL_TRIGGER_CASE.replace(
        '      exact_count: 1',
        '      exact_count: 1\n      body_absent: [metadata]',
      ),
    );
    expect(loadTriggerCase(path).expectations.http[0]?.body_absent).toEqual(['metadata']);
  });

  it('accepts the known vocabulary', () => {
    const spec = loadTriggerCase(writeTemp(MINIMAL_TRIGGER_CASE));
    expect(spec.name).toBe('strictness probe');
    expect(spec.trigger_calls).toHaveLength(1);
  });
});
