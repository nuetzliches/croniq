#!/bin/sh
set -e

DATA_DIR="${CRONIQ_DATA_DIR:-/var/lib/croniq}"

# If we're root (first entrypoint invocation), fix data-dir ownership if
# needed and re-exec ourselves as the croniq user via gosu. Named volumes
# carried over from older root-based images end up owned by root; this
# ensures the croniq user can still write the SQLite database and jwt.secret.
if [ "$(id -u)" = "0" ]; then
  mkdir -p "$DATA_DIR"
  if [ "$(stat -c '%U' "$DATA_DIR")" != "croniq" ]; then
    chown -R croniq:croniq "$DATA_DIR"
  fi
  exec gosu croniq:croniq "$0" "$@"
fi

# Resolve credential env vars from their `<VAR>_FILE` sibling when the
# direct var is unset/empty (Docker/Compose/Swarm secrets, K8s mounted
# Secret volumes, external secret managers). The value never enters the
# environment via the image config, so `docker inspect` shows only a path.
# `$(cat …)` strips the trailing newline secret tooling commonly appends.
load_secret_file() {
  _var="$1"
  eval "_cur=\${$_var:-}"
  eval "_file=\${${_var}_FILE:-}"
  if [ -z "$_cur" ] && [ -n "$_file" ]; then
    if [ ! -r "$_file" ]; then
      echo "ERROR: ${_var}_FILE points to '$_file' which is not readable." >&2
      exit 1
    fi
    export "$_var=$(cat "$_file")"
  fi
}

load_secret_file CRONIQ_ADMIN_PASSWORD
load_secret_file CRONIQ_INIT_API_KEY

# `CRONIQ_ADMIN_PASSWORD` and `CRONIQ_INIT_API_KEY` are *seed* credentials:
# `croniq init` reads them on the very first start and nothing reads them
# again. That is the right behaviour — re-applying a bootstrap credential on
# every start would be worse — but the silence costs something, because the
# value keeps living in whatever holds the deployment's configuration. From
# there, "matches what is in force", "was rotated in the app months ago" and
# "was never right, the DB was seeded with the generated fallback" are
# indistinguishable, and all three appear to work (issue #530). The entrypoint
# is the only component that ever sees these variables, so it is the only place
# that can say so.
warn_seed_only() {
  echo "NOTE: $1 is set but the database already exists;" >&2
  echo "      it is only read when seeding a new database and is ignored here." >&2
  echo "      $2" >&2
}

DB_FILE="$DATA_DIR/croniq.db"

# Auto-initialize on first run if DB doesn't exist
if [ ! -f "$DB_FILE" ]; then
  echo "First run detected — initializing database..."
  ADMIN_USER="${CRONIQ_ADMIN_USER:-admin}"

  if [ -n "$CRONIQ_ADMIN_PASSWORD" ]; then
    ADMIN_PASS="$CRONIQ_ADMIN_PASSWORD"
    PASS_GENERATED=0
    # `croniq init` enforces the server-wide password policy (8–72 bytes,
    # issue #428) and fails with a clear message, so length is not
    # re-checked here — one source of truth. This guard stays for the
    # classic weak value, which the length rule alone would not catch if
    # the policy ever loosened.
    if [ "$ADMIN_PASS" = "admin" ]; then
      echo "ERROR: CRONIQ_ADMIN_PASSWORD='admin' is not accepted." >&2
      echo "       Set a strong password of at least 8 characters." >&2
      echo "       The demo stack uses 'demo-admin' (see docker-compose.yml)." >&2
      exit 1
    fi
  else
    ADMIN_PASS="$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 24)"
    PASS_GENERATED=1
  fi

  # CRONIQ_DEMO_MFA is read directly by `croniq init` via clap's env
  # mapping — no need to translate it into a CLI flag here. But warn
  # loudly when it slips into a non-demo image, since the seed bakes a
  # fixed recovery code (`123456`) into the database.
  if [ "${CRONIQ_DEMO_MFA:-0}" = "1" ] && [ "${CRONIQ_DEMO_MODE:-0}" != "1" ]; then
    echo "WARNING: CRONIQ_DEMO_MFA=1 set without CRONIQ_DEMO_MODE=1." >&2
    echo "         The admin user will be seeded with the recovery code '123456'." >&2
    echo "         Unset CRONIQ_DEMO_MFA before deploying anywhere reachable from the internet." >&2
  fi

  # Build init args as positional params to preserve values with spaces/special chars.
  # Run in a subshell so set -- does not clobber the entrypoint's own "$@".
  # Capture the exit status explicitly so a failure (e.g. malformed
  # CRONIQ_INIT_API_KEY) crash-loops the container instead of leaving
  # the server up with a half-initialized DB and no working API key.
  set +e
  (
    set -- --data-dir "$DATA_DIR" --username "$ADMIN_USER" --password "$ADMIN_PASS"
    if [ -n "$CRONIQ_INIT_API_KEY" ]; then
      set -- "$@" --api-key "$CRONIQ_INIT_API_KEY"
    fi
    croniq init "$@"
  )
  init_status=$?
  set -e
  if [ "$init_status" -ne 0 ]; then
    echo "" >&2
    echo "ERROR: 'croniq init' failed with exit status $init_status." >&2
    if [ -n "$CRONIQ_INIT_API_KEY" ] && \
       ! printf '%s' "$CRONIQ_INIT_API_KEY" | grep -q '^croniq_'; then
      echo "       CRONIQ_INIT_API_KEY must start with 'croniq_' (e.g." >&2
      echo "       CRONIQ_INIT_API_KEY=croniq_\$(openssl rand -hex 32))." >&2
    fi
    # Remove any half-initialized DB so a corrected restart starts cleanly.
    rm -f "$DB_FILE"
    exit "$init_status"
  fi

  if [ "$PASS_GENERATED" = "1" ]; then
    echo ""
    echo "================================================================"
    echo "  Generated admin credentials (shown only once — save them!)"
    echo "  Username: $ADMIN_USER"
    echo "  Password: $ADMIN_PASS"
    echo "================================================================"
    echo "  Set CRONIQ_ADMIN_PASSWORD to use a fixed password instead."
    echo ""
  fi
else
  if [ -n "$CRONIQ_ADMIN_PASSWORD" ]; then
    warn_seed_only CRONIQ_ADMIN_PASSWORD \
      "Rotate it under Settings in the UI, or via POST /v1/users/me/change-password."
  fi
  if [ -n "$CRONIQ_INIT_API_KEY" ]; then
    warn_seed_only CRONIQ_INIT_API_KEY \
      "Manage API keys under Settings → API Keys in the UI, or via 'croniq api-keys'."
  fi
fi

# Demo mode refuses to bind a non-loopback address (issue #431), because the
# profile ships publicly known credentials. Inside a container that refusal
# would be wrong: the process has its own network namespace, so it must bind
# 0.0.0.0 for a published port to reach it at all, and what decides exposure is
# the host-side publish — which docker-compose.yml pins to 127.0.0.1. This
# entrypoint only ever runs inside the image, so it is the right place to say
# so. Exported here and nowhere else: a demo-mode server started directly on a
# host still gets the hard refusal.
if [ "${CRONIQ_DEMO_MODE:-0}" = "1" ]; then
  export CRONIQ_DEMO_CONTAINER_BIND=1
fi

exec "$@"
