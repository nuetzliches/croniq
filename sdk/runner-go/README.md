# Croniq Runner SDK (Go)

Runner SDK with gRPC streaming (primary) and HTTP polling fallback. Use `Runner` for transport chaining, or `Client` for direct HTTP calls.

## Requirements

- Go 1.22+

## Usage

See the sample in [samples/runners/go/polling-basic](../../samples/runners/go/polling-basic).

## Notes

- This client currently polls `/work/poll`, sends events, and acks leases.
- For long-running work, call `Renew` periodically.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.
