module github.com/nuetzliches/croniq/sdks/go/otel

go 1.26.0

// OTel deps track the latest available minor, and each OTel minor raises
// its own Go requirement on its own cadence. OTel 1.47 needs Go 1.26, so
// this adapter's floor (and go.work's, and CI's toolchain) is 1.26 while
// the stdlib-only core SDK in sdks/go/go.mod stays at 1.25. Go 1.25 left
// upstream support when 1.27 shipped (2026-08). When the next OTel minor
// raises the requirement again, move this floor with it.
//
// Cross-module local development is handled by `go.work` at the repo
// root — its `use ./sdks/go` line resolves the parent SDK to the
// in-tree source even though the require below pins a released
// version. The release workflow forbids any `replace` directive in
// this file (see sdks/go/RELEASING.md), so dev-only redirects must
// stay in go.work, not here.
require (
	github.com/nuetzliches/croniq/sdks/go v0.6.2
	go.opentelemetry.io/otel v1.47.0
	go.opentelemetry.io/otel/sdk v1.47.0
	go.opentelemetry.io/otel/trace v1.47.0
)

require (
	github.com/cespare/xxhash/v2 v2.3.0 // indirect
	github.com/go-logr/logr v1.4.4 // indirect
	github.com/go-logr/stdr v1.2.2 // indirect
	github.com/google/uuid v1.6.0 // indirect
	go.opentelemetry.io/auto/sdk v1.2.1 // indirect
	go.opentelemetry.io/otel/log v1.47.0 // indirect
	go.opentelemetry.io/otel/metric v1.47.0 // indirect
	golang.org/x/sys v0.48.0 // indirect
)
