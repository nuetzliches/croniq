# Croniq Job Policies

Policies control retries, timeouts, dead-lettering, and misfire handling. Configure them via `Croniq:Policies:*` in configuration. A fluent per-job policy builder is not available yet.

## Global Defaults

`Croniq:Policies:Execution` sets retry/timeout/circuit-breaker/dead-letter defaults. `Croniq:Policies:Misfire` configures misfire handling.

```json
{
  "Croniq": {
    "Policies": {
      "Execution": {
        "Retry": {
          "Enabled": true,
          "MaxAttempts": 5,
          "BackoffStrategy": "Exponential",
          "InitialDelay": "00:00:02",
          "MaxDelay": "00:00:30"
        },
        "Timeout": {
          "Enabled": true,
          "Timeout": "00:02:00",
          "CancelExecutionOnTimeout": true
        },
        "CircuitBreaker": {
          "Enabled": false
        },
        "DeadLetter": {
          "Enabled": true,
          "Retention": "30.00:00:00"
        }
      },
      "Misfire": {
        "MaxMisfireDelay": "00:05:00",
        "DeadLetterOnMisfire": true,
        "RescheduleBackoff": "00:00:30"
      }
    }
  }
}
```

## Per-job Overrides

Use `Croniq:Policies:Overrides` to override execution/misfire options for specific scopes and to define quota limits. Execution/misfire overrides pick the most specific match (tenant/env/namespace/job). Quotas choose the most restrictive values.

```json
{
  "Croniq": {
    "Policies": {
      "Overrides": {
        "Execution": [
          {
            "NamespaceSegment": "samples",
            "JobName": "smoke",
            "Options": {
              "Timeout": { "Timeout": "00:00:30" }
            }
          }
        ],
        "Quotas": [
          {
            "NamespaceSegment": "samples",
            "JobName": "smoke",
            "Options": {
              "MaxParallelExecutionsPerJob": 2,
              "MaxTriggersPerMinute": 10
            }
          }
        ]
      }
    }
  }
}
```

## Diagnostics

- Use `logging.AddCroniqExecutionLogSink()` or `services.AddCroniqObservability(...)` to capture structured execution logs.
- Misfires and quota reschedules are emitted as metrics via `Croniq.Core.Scheduler` (OTel).

## Dead Letter Replay

When a scheduled trigger is routed to the dead-letter store, list and replay it via the API:

```bash
GET /tenants/{tenantId}/schedules/deadletters?environment={env}
POST /tenants/{tenantId}/schedules/deadletters/{id}/replay?environment={env}
```

The replay endpoint requires the `schedules:deadletter` scope. Replay keeps the stored metadata and adds `deadletter:id` and `deadletter:replay_at` so jobs can trace replays.

See the deep dive in `docs/deep-dive/policies.md` for more background on dead-letter behavior and retention.
