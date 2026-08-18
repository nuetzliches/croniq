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
fi

exec "$@"
