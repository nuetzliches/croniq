# ── Stage 1: Build Rust binaries ──────────────────────────────────────────────
FROM rust:1.88-bookworm AS rust-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build release binaries. `--features croniq-server/otlp,croniq-server/smtp`
# compiles two optional layers into the server:
#   * otlp (issue #121) -- OTLP exporter, honours OTEL_EXPORTER_OTLP_ENDPOINT
#     at runtime. The gate in src/telemetry.rs::decide keeps it dormant when
#     the env var is unset, same behaviour as the off-build.
#   * smtp (PR-A6) -- lettre-backed sender for invitation + password-reset
#     emails. When CRONIQ_SMTP_URL is unset at runtime the NoopSender stays
#     active and the API keeps returning the token URL in the JSON response,
#     so the off-build behaviour is preserved.
RUN cargo build --release \
      --features croniq-server/otlp,croniq-server/smtp \
      --bin croniq-server \
      --bin croniq \
      --bin croniq-mcp \
      --bin croniq-demo-runner \
      --bin croniq-shell-runner

# ── Stage 1b: Build the croniq-config-wasm bridge ────────────────────────────
# WASM output is platform-independent, so we pin this stage to BUILDPLATFORM
# and reuse the artefacts across every TARGETPLATFORM. wasm-pack itself is
# fetched as a pre-built binary (cargo install wasm-pack would add ~2 min).
FROM --platform=$BUILDPLATFORM rust:1.88-bookworm AS wasm-builder

ARG WASM_PACK_VERSION=0.13.1
RUN set -eux; \
    case "$(uname -m)" in \
      x86_64)  arch=x86_64 ;; \
      aarch64) arch=aarch64 ;; \
      *) echo "unsupported build arch: $(uname -m)" >&2; exit 1 ;; \
    esac; \
    url="https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/wasm-pack-v${WASM_PACK_VERSION}-${arch}-unknown-linux-musl.tar.gz"; \
    curl -fsSL "$url" \
      | tar xz -C /usr/local/bin --strip-components=1 --wildcards '*/wasm-pack'; \
    wasm-pack --version

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cd crates/croniq-config-wasm \
    && wasm-pack build --target web --release --out-dir pkg

# ── Stage 2: Build React UI ──────────────────────────────────────────────────
FROM node:24-bookworm-slim AS ui-builder

WORKDIR /build/ui
COPY ui/package.json ui/package-lock.json ./
# `npm ci` enforces the lockfile; fall back to `npm install` only when the
# lockfile is out of sync (e.g. mid-bump). The previous `--frozen-lockfile`
# flag is a yarn/pnpm option — npm silently ignores it, so the lockfile was
# never actually enforced.
RUN npm ci || npm install
COPY ui/ .

# Drop the pre-built WASM bridge into ui/src/lib/wasm/ so the prebuild
# hook (build-wasm.sh) sees fresh artefacts and skips the wasm-pack
# step. Without this the prebuild hook fails because wasm-pack isn't
# installed in node:bookworm-slim.
COPY --from=wasm-builder \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm.js \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm.d.ts \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm_bg.wasm \
    /build/crates/croniq-config-wasm/pkg/croniq_config_wasm_bg.wasm.d.ts \
    src/lib/wasm/

RUN npm run build

# ── Stage 3: Runtime image ───────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 gosu && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd -r croniq && useradd -r -g croniq -s /sbin/nologin croniq

# Copy Rust binaries
COPY --from=rust-builder /build/target/release/croniq-server /usr/local/bin/croniq-server
COPY --from=rust-builder /build/target/release/croniq /usr/local/bin/croniq
COPY --from=rust-builder /build/target/release/croniq-mcp /usr/local/bin/croniq-mcp
COPY --from=rust-builder /build/target/release/croniq-demo-runner /usr/local/bin/croniq-demo-runner
COPY --from=rust-builder /build/target/release/croniq-shell-runner /usr/local/bin/croniq-shell-runner

# Copy UI static files
COPY --from=ui-builder /build/ui/dist /usr/share/croniq/ui

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

# Entrypoint runs as root, fixes data-dir ownership if needed, then
# drops privileges to the croniq user via gosu. This handles upgrades
# from older images where the named volume is owned by root.
ENTRYPOINT ["docker-entrypoint.sh"]
# `--data-dir` deliberately omitted — the server reads `$CRONIQ_DATA_DIR`
# (set above) via clap's `env =` fallback, so a `docker run -e
# CRONIQ_DATA_DIR=…` override stays consistent between the entrypoint's
# first-run init and the server itself. Hardcoding the path would silently
# diverge.
CMD ["croniq-server", "--config", "/etc/croniq/Croniqfile", "--listen", ":4000", "--ui-dir", "/usr/share/croniq/ui"]
