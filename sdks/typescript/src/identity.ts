import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir, hostname } from 'node:os';
import { join } from 'node:path';

import type { Logger } from './logger.js';

export interface IdentityResolverInput {
  /** Explicit runner-id from configuration. */
  runnerId?: string | undefined;
  /** Prefix used when generating a fresh id. */
  runnerIdPrefix: string;
  /** Optional data dir override. Falls back to env var, then platform default. */
  runnerDataDir?: string | undefined;
}

/**
 * Resolves the runner ID with the same precedence as the .NET / Rust SDKs:
 *
 *  1. Explicit option
 *  2. `RUNNER_ID` env var
 *  3. Persisted `runner-id` state file under {@link IdentityResolverInput.runnerDataDir}
 *     (or `CRONIQ_RUNNER_DATA_DIR`, or the platform default)
 *  4. Newly generated `{prefix}-{hex8}`, persisted to the state file
 *
 * If persistence fails (filesystem read-only etc.), returns a deterministic
 * `{prefix}-{hostname}` fallback so the runner can still start.
 */
export function resolveRunnerId(input: IdentityResolverInput, logger: Logger): string {
  if (input.runnerId && input.runnerId.length > 0) return input.runnerId;

  const fromEnv = process.env.RUNNER_ID;
  if (fromEnv && fromEnv.length > 0) return fromEnv;

  const dataDir = resolveDataDir(input.runnerDataDir);
  const idFile = join(dataDir, 'runner-id');

  try {
    if (existsSync(idFile)) {
      const persisted = readFileSync(idFile, 'utf8').trim();
      if (persisted.length > 0) return persisted;
    }
  } catch (err) {
    logger.warn(`could not read persisted runner ID from ${idFile}`, { error: String(err) });
  }

  const generated = `${input.runnerIdPrefix}-${randomBytes(4).toString('hex')}`;
  try {
    mkdirSync(dataDir, { recursive: true });
    writeFileSync(idFile, generated, 'utf8');
  } catch (err) {
    logger.warn(`could not persist generated runner ID to ${idFile}`, { error: String(err) });
    return `${input.runnerIdPrefix}-${hostname().toLowerCase()}`;
  }

  return generated;
}

function resolveDataDir(override?: string): string {
  if (override && override.length > 0) return override;
  const fromEnv = process.env.CRONIQ_RUNNER_DATA_DIR;
  if (fromEnv && fromEnv.length > 0) return fromEnv;

  // Platform-appropriate state directory.
  // On Linux/macOS use $XDG_STATE_HOME or ~/.local/state; on Windows use %LOCALAPPDATA%.
  if (process.platform === 'win32') {
    const localAppData = process.env.LOCALAPPDATA ?? join(homedir(), 'AppData', 'Local');
    return join(localAppData, 'croniq-runner');
  }
  const xdgStateHome = process.env.XDG_STATE_HOME;
  const base = xdgStateHome && xdgStateHome.length > 0
    ? xdgStateHome
    : join(homedir(), '.local', 'state');
  return join(base, 'croniq-runner');
}
