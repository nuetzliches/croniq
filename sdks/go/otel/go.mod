module github.com/nuetzliches/croniq/sdks/go/otel

go 1.22

// OTel deps pinned to v1.32.x — the last minor that supports Go 1.22.
// Bumping past v1.32 would break consumers on Go 1.22.x because OTel
// 1.33+ raised its minimum Go to 1.23, and 1.43+ raised it to 1.25.
// If we ever bump the SDK's minimum Go, lift this floor in lock-step.
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
