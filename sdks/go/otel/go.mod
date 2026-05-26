module github.com/nuetzliches/croniq/sdks/go/otel

go 1.25

// OTel deps track the latest available minor — the project's Go 1.25
// floor (matching go.work and sdks/go/go.mod) lifts the previous 1.22
// constraint that capped us at OTel 1.32.x. If the Go floor ever
// changes again, re-evaluate the OTel version in lock-step (each OTel
// minor raises its own Go requirement on its own cadence).
//
// Cross-module local development is handled by `go.work` at the repo
// root — its `use ./sdks/go` line resolves the parent SDK to the
// in-tree source even though the require below pins a released
// version. The release workflow forbids any `replace` directive in
// this file (see sdks/go/RELEASING.md), so dev-only redirects must
// stay in go.work, not here.
require (
	github.com/nuetzliches/croniq/sdks/go v0.1.0
	go.opentelemetry.io/otel v1.32.0
	go.opentelemetry.io/otel/sdk v1.32.0
	go.opentelemetry.io/otel/trace v1.32.0
)

require (
	github.com/go-logr/logr v1.4.2 // indirect
	github.com/go-logr/stdr v1.2.2 // indirect
	github.com/google/uuid v1.6.0 // indirect
	go.opentelemetry.io/otel/metric v1.32.0 // indirect
	golang.org/x/sys v0.27.0 // indirect
)
