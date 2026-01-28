# Checklist: Runner/Job Assignment (1:1)

## Decisions

- [x] Assignments are 1:1 (JobKey -> RunnerId).
- [x] No `RoutingMode`; only assigned runners receive leases.

## Data model & migrations

- [x] Add assignment fields to job definition/entity (`AssignedRunnerId`, `AssignedBy`, `AssignedAtUtc`, `AssignmentSource`, optional notes).
- [x] Add index on (TenantId, EnvironmentTag, AssignedRunnerId) for runner views.
- [x] Add EF Core migrations for SqlServer + Postgres (include `*.Designer.cs`).
- [x] Enforce "active job must have AssignedRunnerId" validation in API.

## API & backend behavior

- [x] Runner self-registration sets `AssignedRunnerId` and keeps the job pending.
- [x] Reject runner self-registration if the job is active and assigned to a different runner.
- [x] Approving a job also confirms the assignment (sets `IsActive` + assignment approval metadata).
- [x] Add API support to assign/reassign a runner in the job upsert/approval flow.
- [x] Enforce reassignment only when the job is inactive.
- [x] Update dispatch acquisition to only lease jobs where `AssignedRunnerId == RunnerId`.
- [x] Ensure `work/poll` and gRPC dispatch share the same filter.

## UI/UX

- [ ] Job detail: show assigned runner, assignment/approval status, and actions.
- [ ] Runner detail: list assigned jobs.
- [ ] Job approval flow binds the runner automatically.
- [ ] No bulk approval page (explicitly out of scope).

## Tests

- [ ] Unit tests for acquisition filtering by `AssignedRunnerId`.
- [ ] Integration tests for job approval -> assignment confirmation.
- [ ] UI tests for assignment visibility and reassignment.

## Documentation

- [x] Update `docs/deep-dive/architecture.md` (done).
- [ ] Update user docs for job assignment and note that RunnerPool is required for scale-out.
