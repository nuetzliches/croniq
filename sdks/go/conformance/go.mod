// The conformance harness is its own module, on purpose.
//
// It lives under sdks/go but is NOT part of the published
// github.com/nuetzliches/croniq/sdks/go module. Its loaders parse the
// shared YAML fixtures in sdks/conformance/cases, so it needs
// gopkg.in/yaml.v3 — and while it was a plain package of the core module,
// that `require` sat in the published go.mod. Every consumer of the SDK
// inherited a dependency none of their code can ever reach: nothing in the
// library imports the conformance package, so yaml.v3 was dead weight in
// their module graph and live surface in their advisory scans.
//
// Splitting the module drops it: the core SDK's go.mod now has no
// requirements at all (stdlib only), which is the honest description of
// what a consumer links.
//
// This module is never tagged or published — the release workflow only
// knows sdks/go/v* and sdks/go/otel/v* tags — so the `replace` below is
// safe here in a way it would not be in otel/go.mod. It is also load
// bearing rather than a convenience: without it, `go mod tidy` (which
// deliberately ignores go.work) resolves the core SDK from the proxy, and
// the published v0.1.0 still carries its own ./conformance directory, so
// this module's import path resolves in two places at once and tidy fails
// with "ambiguous import". Pointing at the parent directory picks the
// in-tree source, where ./conformance is a nested module and therefore not
// part of the parent — no ambiguity. Ordinary builds and tests resolve the
// same way through the repo-root go.work.
module github.com/nuetzliches/croniq/sdks/go/conformance

go 1.25

require (
	github.com/nuetzliches/croniq/sdks/go v0.1.0
	gopkg.in/yaml.v3 v3.0.1
)

replace github.com/nuetzliches/croniq/sdks/go => ../
