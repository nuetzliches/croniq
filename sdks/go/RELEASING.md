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

Two modules instead of one is deliberate: the core stays
dependency-light (single yaml dep in the conformance suite); only users
who want tracing pull in the OpenTelemetry SDK.

## Bootstrapping: the first release

The otel sub-module's [`go.mod`](otel/go.mod) carries
`require github.com/nuetzliches/croniq/sdks/go vX.Y.Z` — a version that
must exist on `proxy.golang.org` before downstream consumers can resolve
the otel package. That sets up an ordering constraint for the first
release.

### 1. Release `sdks/go/vX.Y.Z` (core)

```sh
# From main, after the SDK PR has merged and all CI is green:
git checkout main && git pull
git tag sdks/go/v0.1.0
git push origin sdks/go/v0.1.0
```

The `Go SDK Release` workflow re-validates the tag, warms
`proxy.golang.org`, and creates a GitHub Release.

Wait until `proxy.golang.org/github.com/nuetzliches/croniq/sdks/go/@v/v0.1.0.info`
returns HTTP 200 (a few seconds, usually).

### 2. Bump the otel module's core requirement

The otel module ships with a `replace github.com/nuetzliches/croniq/sdks/go => ../`
directive so the in-repo source resolves during local development. The
release workflow rejects any otel tag that still carries this directive
— removing it is part of the otel release procedure.

```sh
git checkout main
# 1. Remove the replace directive from sdks/go/otel/go.mod
#    (everything from `// During in-repo development...` to the
#    `replace github.com/nuetzliches/croniq/sdks/go => ../` line)
# 2. Update the require version to the core release you just cut
#    (sed below works for the common case; verify with `git diff`)
sed -i.bak 's|github.com/nuetzliches/croniq/sdks/go v[0-9.]\+|github.com/nuetzliches/croniq/sdks/go v0.1.0|' \
    sdks/go/otel/go.mod
rm sdks/go/otel/go.mod.bak
( cd sdks/go/otel && go mod tidy && go test ./... )

git add sdks/go/otel/go.mod sdks/go/otel/go.sum
git commit -m "chore(go-sdk): drop replace + pin core sdks/go v0.1.0 for otel release"
git push
```

### 3. Restore the replace directive on `main`

Future contributors editing the core SDK need the replace back so their
changes are visible to the otel module without round-tripping through
the proxy. Open a follow-up PR that re-adds it.

```diff
 require (
     github.com/nuetzliches/croniq/sdks/go v0.1.0
     ...
 )
+
+// During in-repo development, resolve the parent SDK from the parent
+// directory rather than the (yet-to-be-tagged) module cache version.
+replace github.com/nuetzliches/croniq/sdks/go => ../
```

This back-and-forth at every release is the only viable workflow for a
sub-module that has a dependency on its parent. The release workflow
fails fast if the replace ships in a release commit, so the cost of
forgetting step 3 is "next release is blocked", not "downstream users
break".

### 4. Release `sdks/go/otel/vX.Y.Z`

```sh
git checkout <commit-from-step-2>   # the one without `replace`
git tag sdks/go/otel/v0.1.0
git push origin sdks/go/otel/v0.1.0
```

The `Go SDK Release` workflow validates, warms the proxy, and creates a
GitHub Release with `go get` instructions.

## Subsequent releases

For releases that don't change the cross-module require (e.g.
patch-level fixes that don't touch the otel module's `go.mod`):

```sh
# Core-only patch:
git tag sdks/go/v0.1.1
git push origin sdks/go/v0.1.1

# OTel-only patch (no need to bump core's tag):
git tag sdks/go/otel/v0.1.1
git push origin sdks/go/otel/v0.1.1
```

For releases that need the otel module to track a new core release,
repeat steps 1 → 4.

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
