# Croniq Runner SDK (Go)

Minimal HTTP polling client for Croniq runners. This package is a starting point until the gRPC-first runner implementation lands.

## Requirements

- Go 1.22+

## Usage

See the sample in [samples/runners/go/polling-basic](../../samples/runners/go/polling-basic).

## Notes

- This client currently polls `/work/poll`, sends events, and acks leases.
- For long-running work, call `Renew` periodically.
- `CRONIQ_RUNNER_ID` must match the API client id associated with the API key.
