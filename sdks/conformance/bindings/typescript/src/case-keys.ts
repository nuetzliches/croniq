// The exact key vocabulary this binding implements, one frozen list per node a
// case YAML nests. `assertKnownKeys` turns anything else into a load-time
// error.
//
// Why not validate against schema/case-schema.json here: CI already does that
// for the whole corpus (the `Conformance YAML schema` job runs
// check-jsonschema against both schemas), and it answers a different question.
// Schema validation catches a key the *schema* does not allow. These lists
// catch a schema-legal key the *binding* has not implemented — the case #460
// was filed for, where a new assertion key loads cleanly everywhere and is
// silently not asserted by the bindings that never implemented it. Adding ajv
// here would duplicate the first check and still leave the second hole open.
//
// The lists therefore mirror the interfaces in case-spec.ts / trigger-case-spec.ts,
// and are expected to lag the schema wherever a capability is .NET-only:
// runner_config's `max_consecutive_poll_conflicts` is in the schema but absent
// here, because the TypeScript SDK has no such option. A case using it must
// fail loudly rather than run with the option ignored.

export const CASE_KEYS = [
  'name',
  'description',
  'runner_config',
  'handlers',
  'server_script',
  'shutdown_after_ms',
  'expectations',
] as const;

export const RUNNER_CONFIG_KEYS = [
  'runner_id',
  'runner_id_prefix',
  'capabilities',
  'tags',
  'max_inflight',
  'api_key',
  'bearer_token',
  'poll_timeout_ms',
  'renew_interval_ms',
  'drain_timeout_ms',
  'poll_retry_delay_ms',
  'capacity_backoff_ms',
] as const;

export const HANDLER_KEYS = [
  'job_key',
  'is_default',
  'schedule',
  'behavior',
  'error_message',
  'duration_ms',
  'level',
  'message',
  'count',
  'interval_ms',
] as const;

export const SCRIPT_ENTRY_KEYS = ['on', 'match_count', 'respond'] as const;
export const RESPOND_KEYS = ['status', 'body', 'delay_ms', 'headers'] as const;
export const EXPECTATIONS_KEYS = ['duration_max_ms', 'http'] as const;

export const HTTP_EXPECTATION_KEYS = [
  'method',
  'path',
  'exact_count',
  'min_count',
  'max_count',
  'headers',
  'body_match',
] as const;

/**
 * Trigger cases additionally pin the omission of unset optionals. Runner cases
 * must not use `body_absent` — case-schema.json does not declare it.
 */
export const TRIGGER_HTTP_EXPECTATION_KEYS = [...HTTP_EXPECTATION_KEYS, 'body_absent'] as const;

export const TRIGGER_CASE_KEYS = [
  'name',
  'description',
  'trigger_config',
  'trigger_calls',
  'server_script',
  'expectations',
] as const;

export const TRIGGER_CONFIG_KEYS = ['api_key', 'bearer_token'] as const;
export const TRIGGER_CALL_KEYS = ['request', 'expect'] as const;

export const TRIGGER_REQUEST_KEYS = [
  'job_key',
  'require',
  'prefer',
  'metadata',
  'timeout',
  'idempotency_key',
] as const;

export const TRIGGER_EXPECT_KEYS = ['response', 'error'] as const;

/**
 * Asserted one key at a time in the trigger suite, so an unrecognised key here
 * is the silent-drop case exactly — it has to be rejected up front.
 */
export const TRIGGER_RESPONSE_KEYS = ['execution_id', 'queued', 'deduplicated'] as const;

/**
 * Reject any key `allowed` does not list.
 *
 * Non-objects pass through: the loaders assert the overall shape, and a
 * missing optional block is the corpus's business (and CI's schema job), not
 * this check's.
 */
export function assertKnownKeys(node: unknown, allowed: readonly string[], ctx: string): void {
  if (node === null || typeof node !== 'object' || Array.isArray(node)) return;
  const unknown = Object.keys(node).filter((key) => !allowed.includes(key));
  if (unknown.length > 0) {
    throw new Error(
      `${ctx}: unrecognised key(s) ${JSON.stringify(unknown.sort())}. ` +
        `This binding does not implement them — either the case is wrong or the ` +
        `TypeScript conformance binding needs updating. Known keys: ` +
        `${JSON.stringify([...allowed].sort())}`,
    );
  }
}
