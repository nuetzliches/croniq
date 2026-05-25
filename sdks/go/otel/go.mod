module github.com/nuetzliches/croniq/sdks/go/otel

go 1.22

require (
	github.com/nuetzliches/croniq/sdks/go v0.1.0
	go.opentelemetry.io/otel v1.43.0
	go.opentelemetry.io/otel/sdk v1.43.0
	go.opentelemetry.io/otel/trace v1.43.0
)

require (
	github.com/cespare/xxhash/v2 v2.3.0 // indirect
	github.com/go-logr/logr v1.4.3 // indirect
	github.com/go-logr/stdr v1.2.2 // indirect
	github.com/google/uuid v1.6.0 // indirect
	go.opentelemetry.io/auto/sdk v1.2.1 // indirect
	go.opentelemetry.io/otel/metric v1.43.0 // indirect
	golang.org/x/sys v0.42.0 // indirect
)

// During in-repo development, resolve the parent SDK from the parent
// directory rather than the (yet-to-be-tagged) module cache version.
//
// Replace directives only take effect in the *main* module — when
// downstream consumers `go get` this otel package, the directive is
// ignored and the `require` version above is what resolves on
// proxy.golang.org. The release workflow at .github/workflows/
// go-sdk-release.yml validates this require line during otel tag
// builds; see sdks/go/RELEASING.md for the bootstrap procedure.
replace github.com/nuetzliches/croniq/sdks/go => ../
