# ── Stage 1: Build Rust binaries ──────────────────────────────────────────────
FROM rust:1.88-bookworm AS rust-builder

# Statically linked against musl, because the runtime below is Alpine (#599).
# Same target release.yml has published since #577, and the same reasoning:
# the C in the closure (`ring`, and the bundled SQLite behind croniq-store)
# has to be compiled by musl's cc, or a nominally-musl build ends up carrying
# glibc headers. The *linker* is deliberately left alone -- overriding it with
# musl-gcc yields a binary with an ELF interpreter, and static-pie is the
# point. The build asserts that below rather than trusting it.
#
# Derived from `uname -m` rather than TARGETPLATFORM: buildx runs this stage
# on a native runner per architecture (see .github/workflows/ci.yml), so the
# build arch *is* the target arch, and there is no aarch64-musl cross
# toolchain in apt to reach for anyway.
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools && \
    rm -rf /var/lib/apt/lists/* && \
    rustup target add "$(uname -m)-unknown-linux-musl"

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build release binaries. `--features croniq-server/{otlp,smtp,postgres}`
# compiles three optional layers into the server:
#   * otlp (issue #121) -- OTLP exporter, honours OTEL_EXPORTER_OTLP_ENDPOINT
#     at runtime. The gate in src/telemetry.rs::decide keeps it dormant when
#     the env var is unset, same behaviour as the off-build.
#   * smtp (PR-A6) -- lettre-backed sender for invitation + password-reset
#     emails. When CRONIQ_SMTP_URL is unset at runtime the NoopSender stays
#     active and the API keeps returning the token URL in the JSON response,
#     so the off-build behaviour is preserved.
#   * postgres -- lets `server { db postgres://… }` / CRONIQ_DB select a
#     PostgreSQL backend at runtime. SQLite stays the default; the feature only
#     adds the (pure-Rust, NoTls) postgres driver, so the runtime image needs no
#     extra system libraries.
#
# The readelf check at the end is not decoration: `file` reports "statically
# linked" even for a musl binary that carries /lib/ld-musl-*.so.1 as its
# interpreter, so it cannot tell the two apart. Such a binary would run fine
# here -- Alpine has musl -- and fail everywhere else the runner binaries get
# shipped to (#577). The comment lives above the RUN rather than inside it: a
# `#` line within a line continuation is a Dockerfile footgun.
RUN set -eux; \
    target="$(uname -m)-unknown-linux-musl"; \
    export "CC_$(echo "$target" | tr - _)=musl-gcc"; \
    cargo build --release --target "$target" \
      --features croniq-server/otlp,croniq-server/smtp,croniq-server/postgres \
      --bin croniq-server \
      --bin croniq \
      --bin croniq-mcp \
      --bin croniq-demo-runner \
      --bin croniq-shell-runner; \
    mkdir -p /out; \
    for b in croniq-server croniq croniq-mcp croniq-demo-runner croniq-shell-runner; do \
      bin="target/$target/release/$b"; \
      if readelf -l "$bin" | grep -q INTERP; then \
        echo "$b needs a dynamic loader - expected static-pie" >&2; exit 1; \
      fi; \
      cp "$bin" /out/; \
    done

# ── Stage 1b: Build the croniq-config-wasm bridge ────────────────────────────
# WASM output is platform-independent, so we pin this stage to BUILDPLATFORM
# and reuse the artefacts across every TARGETPLATFORM. wasm-pack itself is
# fetched as a pre-built binary (cargo install wasm-pack would add ~2 min).
FROM --platform=$BUILDPLATFORM rust:1.88-bookworm AS wasm-builder

ARG WASM_PACK_VERSION=0.13.1
# Pinned SHA256 of the release tarballs — the download is verified before
# extraction so a tampered artefact fails the build instead of shipping.
# When bumping WASM_PACK_VERSION, recompute both:
#   curl -fsSL <release tarball url> | sha256sum
ARG WASM_PACK_SHA256_X86_64=c539d91ccab2591a7e975bcf82c82e1911b03335c80aa83d67ad25ed2ad06539
ARG WASM_PACK_SHA256_AARCH64=2e65038769f8bbaa5fc237ad4bb523e692df99458cbd3e3d92525b89d8762379
RUN set -eux; \
    case "$(uname -m)" in \
      x86_64)  arch=x86_64;  sha256="$WASM_PACK_SHA256_X86_64" ;; \
      aarch64) arch=aarch64; sha256="$WASM_PACK_SHA256_AARCH64" ;; \
      *) echo "unsupported build arch: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    url="https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/wasm-pack-v${WASM_PACK_VERSION}-${arch}-unknown-linux-musl.tar.gz"; \
    curl -fsSL "$url" -o /tmp/wasm-pack.tar.gz; \
    echo "${sha256}  /tmp/wasm-pack.tar.gz" | sha256sum -c -; \
    tar xzf /tmp/wasm-pack.tar.gz -C /usr/local/bin --strip-components=1 --wildcards '*/wasm-pack'; \
    rm /tmp/wasm-pack.tar.gz; \
    wasm-pack --version

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cd crates/croniq-config-wasm \
    && wasm-pack build --target web --release --out-dir pkg

# ── Stage 2: Build the dashboard ─────────────────────────────────────────────
FROM node:24-bookworm-slim AS ui-builder

WORKDIR /build/ui
COPY ui/package.json ui/package-lock.json ./
# `npm ci` enforces the lockfile; fall back to `npm install` only when the
# lockfile is out of sync (e.g. mid-bump). The previous `--frozen-lockfile`
# flag is a yarn/pnpm option — npm silently ignores it, so the lockfile was
# never actually enforced.
RUN npm ci || npm install
COPY ui/ .

# The release this bundle is for, so the dashboard can tell an operator when it
# and the server were not published together (#604). Only the split topology
# can skew — the combined image is one digest — but the stamp is the same for
# all three targets because they come out of this one stage.
#
# Defaults to `dev`, which the dashboard reads as "not stamped, say nothing".
# A plain `docker build .` with no `--build-arg` must not produce a bundle
# claiming a version, and it must not nag either; the release workflow passes
# the tag.
ARG CRONIQ_VERSION=dev
ENV VITE_APP_VERSION=$CRONIQ_VERSION

# Drop the pre-built WASM bridge into ui/app/lib/wasm/ so the prebuild
# hook (build-wasm.mjs) sees fresh artefacts and skips the wasm-pack
# step. Without this the prebuild hook fails because wasm-pack isn't
# installed in node:bookworm-slim.
COPY --from=wasm-builder \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm.js \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm.d.ts \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm_bg.wasm \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm_bg.wasm.d.ts \
    app/lib/wasm/

RUN npm run build

# ── Stage 2b: the CA bundle, and nothing else ────────────────────────────────
# Its own stage rather than a copy out of `rust-builder`, and the difference is
# not cosmetic. `rust:1.88-bookworm` carries whatever `ca-certificates` was
# current when *that image* was built: copying from there swapped 150 roots for
# 142 -- losing 21 and gaining 13. A trust store is not a file you substitute
# casually, and the failure mode is a TLS handshake that works everywhere
# except one customer's Postgres.
#
# Same base as the runtime, same `apt-get update` at build time, so the bundle
# is byte-for-byte what an apt-installed one would have been -- verified by
# sha256 rather than assumed. ci.yml's docker job asserts it on every push to
# main, before anything is published.
FROM debian:bookworm-slim AS ca-provider
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# ── Stage 3: Server runtime, without the dashboard ───────────────────────────────────────────────────
# Buildable on its own (`--target server-runtime`), and in that case buildx
# never touches the ui-builder or wasm-builder stages above — no Node, no npm
# tree, nothing from ui/ in this image's provenance. That separation is the
# point of the target (#598); it is *not* about size, where the dashboard is
# 0.37 MB of 54.79 MB, nor about build time, which the layer cache already
# isolates. See ADR-0002.
#
# The combined image at the bottom of this file extends this stage rather than
# repeating it, so the two cannot drift. That inheritance is also why the
# binary set here is the full one, `croniq-demo-runner` included: dropping it
# here would drop it from the combined image too, and docker-compose.yml's demo
# profile runs it. Which binaries belong where is #599.
FROM alpine:3.22 AS server-runtime

# `su-exec` is Alpine's `gosu`: same argv shape, 10 KB of C against 2 MB of
# Go. The entrypoint calls it by name.
#
# No `ca-certificates` package. Nothing in the closure links OpenSSL -- `ldd`
# on all five binaries lists libgcc_s, libm and libc, because every TLS path
# here is rustls (reqwest for OIDC, lettre for SMTP, tokio-postgres-rustls for
# Postgres). The trust store is still needed, but only the file, and only by
# `rustls-native-certs` behind tokio-postgres-rustls: a Postgres server
# presenting a private CA is the one case that reads it.
RUN apk add --no-cache su-exec && \
    addgroup -S croniq && adduser -S -D -H -G croniq -s /sbin/nologin croniq

# Debian's bundle, deliberately, over the one Alpine already ships.
#
# Alpine's `ca-certificates-bundle` carries 119 roots to Debian bookworm's
# 150; compared by SHA-256 fingerprint, 37 certificates are in Debian and not
# in Alpine -- among them DigiCert Global Root CA, Baltimore CyberTrust Root,
# GlobalSign Root CA and GTS Root R2, which is to say most of what a managed
# Postgres chains to.
#
# Changing the base image must not quietly also change who the product
# trusts. Those are two decisions and only one of them was taken. Keeping the
# bundle constant across the move means the failure mode this would otherwise
# have -- one customer's Postgres stops verifying after an upgrade that said
# nothing about certificates -- simply does not exist. 224 KB.
COPY --from=ca-provider /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Copy Rust binaries
COPY --from=rust-builder /out/croniq-server /usr/local/bin/croniq-server
COPY --from=rust-builder /out/croniq /usr/local/bin/croniq
COPY --from=rust-builder /out/croniq-mcp /usr/local/bin/croniq-mcp
COPY --from=rust-builder /out/croniq-demo-runner /usr/local/bin/croniq-demo-runner
COPY --from=rust-builder /out/croniq-shell-runner /usr/local/bin/croniq-shell-runner

# Copy assets
COPY assets/ /usr/share/croniq/assets/
COPY Croniqfile.example /etc/croniq/Croniqfile

# Data directory — created and owned before VOLUME so named volumes inherit ownership
RUN mkdir -p /var/lib/croniq && chown croniq:croniq /var/lib/croniq
VOLUME /var/lib/croniq

COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

ENV RUST_LOG=info
ENV CRONIQ_DATA_DIR=/var/lib/croniq
EXPOSE 4000 9900

# Entrypoint runs as root, fixes data-dir ownership if needed, then drops
# privileges to the croniq user via su-exec. That covers upgrades from the
# root-based images of v0.4.0 and earlier, and — more durably — any bind mount
# owned by someone the image has never heard of.
ENTRYPOINT ["docker-entrypoint.sh"]
# `--data-dir` deliberately omitted — the server reads `$CRONIQ_DATA_DIR`
# (set above) via clap's `env =` fallback, so a `docker run -e
# CRONIQ_DATA_DIR=…` override stays consistent between the entrypoint's
# first-run init and the server itself. Hardcoding the path would silently
# diverge.
CMD ["croniq-server", "--config", "/etc/croniq/Croniqfile", "--listen", ":4000"]


# ── Stage 4: Dashboard runtime ───────────────────────────────────────────────
# Serves the built bundle and nothing else. It deliberately does **not** proxy
# /v1: the reverse proxy in front routes / here and /v1 to croniq-server, and
# that is what keeps the dashboard same-origin with the API — which ADR-0001
# requires, because the refresh token is a `SameSite=Strict` cookie that cannot
# reach a different origin. A `proxy_pass` in here would add a second,
# undocumented route to the API inside a container whose job is HTML.
#
# Running this image *without* such a proxy is not a deployment variation, it
# is the localStorage exposure #454 removed. docs/operations.md says so.
#
# nginx-unprivileged rather than the official nginx image: this container hands
# static files to a browser and does not need to start as root to do it. It
# runs as UID 101 and listens on 8080, so no capability is needed to bind.
FROM nginxinc/nginx-unprivileged:1.29-alpine AS ui-runtime
COPY docker/ui/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=ui-builder /build/ui/dist /usr/share/nginx/html
EXPOSE 8080


# ── Stage 5: Combined image (the default target) ─────────────────────────────
# Last stage, so a plain `docker build .` still produces exactly what it always
# did: server plus dashboard on one port. This is the supported default for
# quickstart, demo and single-host deployments (ADR-0002), and the split above
# is an option for deployments that already run a reverse proxy — not a
# replacement.
FROM server-runtime AS combined
COPY --from=ui-builder /build/ui/dist /usr/share/croniq/ui
CMD ["croniq-server", "--config", "/etc/croniq/Croniqfile", "--listen", ":4000", "--ui-dir", "/usr/share/croniq/ui"]
