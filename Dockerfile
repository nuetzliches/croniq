# ── Stage 1: Build Rust binaries ──────────────────────────────────────────────
FROM rust:1.87-bookworm AS rust-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build release binaries
RUN cargo build --release --bin croniq-server --bin croniq --bin croniq-mcp

# ── Stage 2: Build React UI ──────────────────────────────────────────────────
FROM node:24-bookworm-slim AS ui-builder

WORKDIR /build/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/ .
RUN npm run build

# ── Stage 3: Runtime image ───────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Copy Rust binaries
COPY --from=rust-builder /build/target/release/croniq-server /usr/local/bin/croniq-server
COPY --from=rust-builder /build/target/release/croniq /usr/local/bin/croniq
COPY --from=rust-builder /build/target/release/croniq-mcp /usr/local/bin/croniq-mcp

# Copy UI static files
COPY --from=ui-builder /build/ui/dist /usr/share/croniq/ui

# Copy assets
COPY assets/ /usr/share/croniq/assets/
COPY Croniqfile.example /etc/croniq/Croniqfile

# Data directory
RUN mkdir -p /var/lib/croniq
VOLUME /var/lib/croniq

ENV RUST_LOG=info
EXPOSE 9090 9900

ENTRYPOINT ["croniq-server"]
CMD ["--config", "/etc/croniq/Croniqfile", "--data-dir", "/var/lib/croniq", "--listen", ":9090"]
