# Releasing the Go SDK

Go modules have **no central registry**. A release IS a git tag —
`proxy.golang.org` indexes the tag on first fetch, and `pkg.go.dev`
surfaces the documentation automatically. There is no separate publish
step.

The Croniq Go SDK ships as **two independent modules**:

| Module path | Tag pattern | Lives in |
|---|---|---|
| `github.com/nuetzliches/croniq/sdks/go` | `sdks/go/vX.Y.Z` | [`sdks/go/`](.) |
| `github.com/nuetzliches/croniq/sdks/go/otel` | `sdks/go/otel/vX.Y.Z` | [`sdks/go/otel/`](otel/) |

Two modules instead of one is deliberate: the core is **stdlib-only**, with
an empty `require` block and an empty `go.sum`; only users who want tracing
pull in the OpenTelemetry SDK.

A third module, [`sdks/go/conformance/`](conformance/), is **never tagged and
never published**. It exists so its `gopkg.in/yaml.v3` requirement — needed
to read the shared conformance fixtures — stays out of the core SDK's
published `go.mod`, where it used to sit even though no importable package
reached it. Do not tag it; the release workflow only recognises
`sdks/go/v*` and `sdks/go/otel/v*`. Because it is unpublished it may (and
does) carry a `replace` pointing at `../`, unlike `otel/go.mod`.

## How cross-module local dev works

The otel sub-module's [`go.mod`](otel/go.mod) carries
`require github.com/nuetzliches/croniq/sdks/go vX.Y.Z` pointing at a
released version. Local development uses [`go.work`](../../go.work) at
the repo root to redirect that require to the in-tree source:

```
use (
    ./sdks/go
    ./sdks/go/conformance
    ./sdks/go/otel
)
```

This means contributors editing the core SDK see their changes from the
otel module immediately, without committing a `replace` directive that
would break downstream `go get` consumers. The release workflow's
validate step **rejects any `replace` directive in
[otel/go.mod](otel/go.mod)** so the asymmetry stays committed in
`go.work`, not in the published module.

Run tests from inside each module directory to use the workspace; the core
SDK resolves to in-tree source automatically from both the otel and the
conformance module. (`go test ./sdks/go/...` from the repo root only covers
the core module — nested modules are excluded from a parent's `./...`.)

## Cutting a release

### Core SDK

```sh
git checkout main && git pull
git tag -a sdks/go/v0.1.0 -m "sdks/go v0.1.0 — short summary"
git push origin sdks/go/v0.1.0
```

The `Go SDK Release` workflow re-validates the tag, warms
`proxy.golang.org`, and creates a GitHub Release.

Wait until
`https://proxy.golang.org/github.com/nuetzliches/croniq/sdks/go/@v/v0.1.0.info`
returns HTTP 200 (the workflow does this for you; the warning at the
end of the validate step is harmless if you see it).

### OTel adapter

Cutting a new otel release means making its `require` line point at a
core version that has already been tagged and indexed by
`proxy.golang.org`. The flow is a single PR:

```sh
git checkout main && git pull
git checkout -b chore/bump-otel-core-require

# Bump the require to the latest core release. `go mod tidy` with
# GOWORK=off mirrors what the release workflow's validate step does.
sed -i 's|github.com/nuetzliches/croniq/sdks/go v[0-9][^ ]*|github.com/nuetzliches/croniq/sdks/go v0.2.0|' \
    sdks/go/otel/go.mod
( cd sdks/go/otel && GOWORK=off go mod tidy && go test ./... )

git add sdks/go/otel/go.mod sdks/go/otel/go.sum
git commit -m "chore(go-sdk): bump otel core require to v0.2.0"
git push -u origin chore/bump-otel-core-require
gh pr create --fill
```

After the PR merges:

```sh
git checkout main && git pull
git tag -a sdks/go/otel/v0.2.0 -m "sdks/go/otel v0.2.0 — short summary"
git push origin sdks/go/otel/v0.2.0
```

If the otel release is independent of any core change (e.g. an
OTel-side bug fix), skip the bump PR — just tag straight off `main`.

## Pre-releases

Use a `-rc1` / `-beta.1` suffix on the version (the workflow flags
these as GitHub pre-releases, so they don't replace "Latest" in the UI):

```sh
git tag sdks/go/v0.2.0-rc1
git push origin sdks/go/v0.2.0-rc1
```

## Yanking a release

There is no "delete from the proxy" — once `proxy.golang.org` has
indexed a tag, the version exists in the immutable module cache forever.
The convention for retracting a buggy release is the `retract` directive
in the **next** release's `go.mod`:

```go
// In sdks/go/go.mod, when v0.1.1 retracts v0.1.0:
retract v0.1.0   // botched lease-renewal — use v0.1.1+
```

Users on `@latest` skip retracted versions automatically; users who
pinned the retracted version see a warning on `go build`.

## Useful URLs

- Proxy module info: `https://proxy.golang.org/{module-path}/@v/{version}.info`
- Go module docs: `https://pkg.go.dev/{module-path}@{version}`
- All tags for this module: `git tag --list 'sdks/go/v*'`
