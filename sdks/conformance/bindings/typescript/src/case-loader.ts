import { readFileSync } from 'node:fs';

import { load } from 'js-yaml';

import type { CaseSpec } from './case-spec.js';

export function loadCase(path: string): CaseSpec {
  const text = readFileSync(path, 'utf8');
  const parsed = load(text) as CaseSpec;
  if (!parsed || typeof parsed !== 'object') {
    throw new Error(`failed to parse conformance case at ${path}`);
  }
  return parsed;
}

/** Split `"METHOD /path"` into (method, path). */
export function splitOn(on: string): { method: string; path: string } {
  const idx = on.indexOf(' ');
  if (idx < 0) throw new Error(`invalid server_script.on rule: ${on}`);
  return { method: on.slice(0, idx).toUpperCase(), path: on.slice(idx + 1) };
}
