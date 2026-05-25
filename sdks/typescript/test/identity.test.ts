import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { resolveRunnerId } from '../src/identity.js';
import { noopLogger } from '../src/logger.js';

describe('resolveRunnerId', () => {
  let dir: string;
  const env = { ...process.env };

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'croniq-id-'));
    delete process.env.RUNNER_ID;
    delete process.env.CRONIQ_RUNNER_DATA_DIR;
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
    process.env = { ...env };
  });

  it('uses explicit runnerId when provided', () => {
    expect(
      resolveRunnerId({ runnerId: 'explicit-id', runnerIdPrefix: 'x', runnerDataDir: dir }, noopLogger),
    ).toBe('explicit-id');
  });

  it('uses RUNNER_ID env var when no explicit option', () => {
    process.env.RUNNER_ID = 'from-env';
    expect(resolveRunnerId({ runnerIdPrefix: 'x', runnerDataDir: dir }, noopLogger)).toBe('from-env');
  });

  it('reads persisted runner-id file', () => {
    writeFileSync(join(dir, 'runner-id'), 'persisted-id\n', 'utf8');
    expect(resolveRunnerId({ runnerIdPrefix: 'x', runnerDataDir: dir }, noopLogger)).toBe('persisted-id');
  });

  it('generates and persists a fresh id when none exists', () => {
    const id = resolveRunnerId({ runnerIdPrefix: 'foo', runnerDataDir: dir }, noopLogger);
    expect(id).toMatch(/^foo-[0-9a-f]{8}$/);
    expect(readFileSync(join(dir, 'runner-id'), 'utf8')).toBe(id);
  });

  it('honors CRONIQ_RUNNER_DATA_DIR env var when no explicit dir', () => {
    process.env.CRONIQ_RUNNER_DATA_DIR = dir;
    writeFileSync(join(dir, 'runner-id'), 'from-env-dir', 'utf8');
    expect(resolveRunnerId({ runnerIdPrefix: 'x' }, noopLogger)).toBe('from-env-dir');
  });

  it('explicit option beats env var', () => {
    process.env.RUNNER_ID = 'from-env';
    expect(
      resolveRunnerId({ runnerId: 'explicit', runnerIdPrefix: 'x', runnerDataDir: dir }, noopLogger),
    ).toBe('explicit');
  });
});
