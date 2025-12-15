# Croniq Workflow Concept

_Last updated: 2025-12-15_

## Objectives

- Enable consumers to orchestrate multi-step job progressions ("workflows") without embedding state machines inside each job.
- Keep Croniq's existing scheduling/execution pipeline intact while layering workflow metadata and guardrails on top.
- Allow tenants to describe domain-specific states (e.g., `draft`, `ready`, `processing`, `done`) and let Croniq enforce the allowed transitions.
- Support extensibility so future hosts (UI, CLI, SDKs) can visualize and operate workflows.

## Terminology

| Term                    | Description                                                                                                                          |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| **Workflow Definition** | Declarative object owned by a tenant/environment describing statuses and transitions.                                                |
| **State**               | Consumer-provided status identifier (string) representing a lifecycle step. Croniq treats it opaquely.                               |
| **Transition**          | Directed edge between two states that may execute Croniq jobs, mutate metadata, and gate movement via policies.                      |
| **Workflow Instance**   | Runtime projection that tracks the current state, history, and payload references for a business entity (order, ticket, deployment). |
| **Trigger Job**         | Existing Croniq job invoked as part of entering or exiting a transition.                                                             |

## Authoring Model

1. **Definition Schema**
   - Tenant scoped: `WorkflowDefinition { WorkflowKey, States[], Transitions[] }`.
   - States: `StateId` (string), optional display metadata.

- Transitions: `TransitionId`, `FromState`, `ToState`, optional ordered `EntryJobs[]` / `ExitJobs[]` collections (each item = `JobKey`, `ExecutionMode` sequential|parallel), policy overrides, SLA metadata.
- Validation ensures states exist, no orphan transitions, and at least one `initial` flag is present.

2. **Storage**

   - Persist definitions via new tables (e.g., `croniq.WorkflowDefinitions`, `croniq.WorkflowTransitions`).
   - Version definitions for safe edits; references from instances include `DefinitionVersion`.

3. **APIs**
   - `POST /tenants/{tenantId}/workflows` to create definitions.
   - `GET /tenants/{tenantId}/workflows/{workflowKey}` to fetch definitions + versions.
   - `POST /tenants/{tenantId}/workflow-instances` to start an instance (assigns entity id, initial state).
   - `POST /tenants/{tenantId}/workflow-instances/{instanceId}/transitions/{transitionId}` to advance state.

## Runtime & Scheduler Integration

- Transition execution composes existing Croniq jobs:
  1. Validate caller authorization (`workflow:*` scopes) and ensure requested transition is allowed from the instance's current state.
  2. If `EntryJobKey` is defined, schedule/trigger it before state change (supports async completion callbacks via existing execution logs).
  3. Update instance state when the job completes successfully; failed executions can dead-letter and keep the instance in its previous state.
  4. Optionally support `ExitJobKey` for cleanup/notifications after state mutation.
- Instances capture history rows: `InstanceId`, `TransitionId`, timestamps, job execution ids, actor/correlation ids.
- Rate limiting & policies reuse tenant scopes; additional workflow-specific policies (e.g., max concurrent transitions) can be layered later.

## Transitions vs. Jobs

- **Unified Feature**: Transitions are first-class workflow constructs that _can_ embed Croniq jobs. They are not a separate feature; instead, they orchestrate one or more existing jobs.
- Each transition can reference ordered job arrays on entry and exit. Execution modes:
  - **Sequential**: Croniq waits for job completion before firing the next item in the list.
  - **Parallel**: Croniq fans out jobs concurrently and aggregates their completion status before advancing the workflow.
- If more complex branching is required, combine sequential and parallel blocks or compose additional transitions (`state A -> state B -> state C`). Wrappers that dispatch further Croniq jobs remain valid but are no longer required for multi-job steps.
- This keeps workflows declarative while reusing the battle-tested job execution pipeline.

## Observability & Telemetry

- Emit OpenTelemetry spans per transition with tags: `workflow.key`, `workflow.instance_id`, `workflow.transition`, `workflow.from_state`, `workflow.to_state`.
  - Link spans to underlying job execution spans via `JobExecutionId` for end-to-end tracing.
- Metrics: counters for transition requests, successes, failures, SLA breaches.
- Logs: structured entries when transitions start/complete plus validation denials.

## Security & Authorization

- Extend `CroniqScopes` with `workflows:read`, `workflows:write`, `workflow-instances:move`.
- Tenant guard ensures instances cannot move across tenant/environment boundaries.
- Workflow definitions and instances respect the same persistence providers (SqlServer initially) with encrypted secrets delegated to existing stores.

## Future Extensions

- **Conditional Logic**: allow transitions to depend on payload predicates or job outputs.
- **Parallel Branches**: support multiple active states by relaxing single-state constraint per instance.
- **UI/Visualization**: once UI work resumes, render workflow graphs and live statuses.
- **SDK Helpers**: typed clients that issue transition commands with optimistic concurrency tokens.

## Open Questions

1. Do we need workflow-wide SLAs (e.g., must finish within N hours) beyond per-transition metrics?
2. Should transitions support custom payload transforms before invoking jobs?
3. Can consumers plug in external approval steps (human-in-the-loop) before Croniq triggers the job?
4. How do we expose workflow state in Croniq's gRPC surface (new RPC vs. HTTP only)?

Document decisions here as the concept evolves; once implementation starts, mirror the finalized plan into `docs/deep-dive/designs/workflows.md` and update the master checklist accordingly.
