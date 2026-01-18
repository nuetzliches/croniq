# Retention

Croniq can optionally schedule a maintenance job that prunes expired operational data from the SqlServer or Postgres persistence store.
This is only wired when the worker host uses `Persistence.Mode = SqlServer` or `Persistence.Mode = Postgres`.

- JobKey: `croniq:retention-cleanup`
- Default trigger id: `croniq.retention.cleanup`

## Configuration

| Key                             | Description                                                                                                |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `Croniq:Retention:Enabled`      | Enables the maintenance trigger. Default `false`.                                                          |
| `Croniq:Retention:ScheduleCron` | 6-field cron expression with optional year used by the trigger. Default `0 0 3 ? * * *` (daily 03:00 UTC). |
| `Croniq:Retention:TimeZoneId`   | Optional time zone id for the trigger schedule. Default UTC.                                               |
| `Croniq:Retention:TriggerId`    | Optional stable trigger id override. Default `croniq.retention.cleanup`.                                   |

### Refresh tokens

| Key                                           | Description                                                                                                                                |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `Croniq:Retention:RefreshTokensEnabled`       | Enables pruning `auth.RefreshTokens`. Default `true`.                                                                                      |
| `Croniq:Retention:RefreshTokensRetentionDays` | Deletes when `ExpiresAtUtc + days < now` for the current tenant. Use `0` for immediate deletion after expiry; `-1` disables. Default `14`. |

### Additional retention tasks (opt-in)

| Key                                                     | Description                                                                                              |
| ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `Croniq:Retention:JobDeadLettersEnabled`                | Enables pruning `croniq.DeadLetters` (scoped to current tenant+environment). Default `false`.            |
| `Croniq:Retention:JobDeadLettersExpiryOffsetDays`       | Deletes when `ExpiresAtUtc + days < now` (use `0` for immediate, `-1` to disable). Default `0`.          |
| `Croniq:Retention:WebhookDeadLettersEnabled`            | Enables pruning `croniq.WebhookDeadLetters` (only rows with non-null `ExpiresAtUtc`). Default `false`.   |
| `Croniq:Retention:WebhookDeadLettersExpiryOffsetDays`   | Deletes when `ExpiresAtUtc + days < now` (use `0` for immediate, `-1` to disable). Default `0`.          |
| `Croniq:Retention:WebhookEndpointEventsEnabled`         | Enables pruning `croniq.WebhookEndpointEvents` using `OccurredAtUtc` as baseline. Default `false`.       |
| `Croniq:Retention:WebhookEndpointEventsRetentionDays`   | Deletes when `OccurredAtUtc + days < now` (use `-1` to disable). Default `30`.                           |
| `Croniq:Retention:WebhookSecretHistoryEnabled`          | Enables pruning `croniq.WebhookSecretHistory` (only rows with non-null `ExpiresAtUtc`). Default `false`. |
| `Croniq:Retention:WebhookSecretHistoryExpiryOffsetDays` | Deletes when `ExpiresAtUtc + days < now` (use `0` for immediate, `-1` to disable). Default `7`.          |
