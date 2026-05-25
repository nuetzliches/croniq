import type { WorkEvent } from './protocol.js';

/**
 * Per-execution helper that injects `job_key`, `runner_id`, and (when set)
 * `runner_tags` into every event's `fields` map. Explicit caller values win
 * — the enricher uses `TryAdd` semantics, so a handler that puts an
 * `runner_id` in its own fields keeps that override.
 *
 * Mirrors `LogEnrichment` from the .NET SDK.
 */
export class LogEnrichment {
  readonly #jobKey: string;
  readonly #runnerId: string;
  readonly #serializedTags: string | undefined;

  constructor(jobKey: string, runnerId: string, runnerTags: readonly string[]) {
    this.#jobKey = jobKey;
    this.#runnerId = runnerId;
    this.#serializedTags = runnerTags.length === 0 ? undefined : JSON.stringify(runnerTags);
  }

  enrich(source: WorkEvent): WorkEvent {
    const fields: Record<string, string> = { ...(source.fields ?? {}) };
    if (!('job_key' in fields)) fields.job_key = this.#jobKey;
    if (!('runner_id' in fields)) fields.runner_id = this.#runnerId;
    if (this.#serializedTags !== undefined && !('runner_tags' in fields)) {
      fields.runner_tags = this.#serializedTags;
    }
    return { ...source, fields };
  }
}
