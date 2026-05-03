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

DB_FILE="$DATA_DIR/croniq.db"

# Auto-initialize on first run if DB doesn't exist
if [ ! -f "$DB_FILE" ]; then
  echo "First run detected — initializing database..."
  ADMIN_USER="${CRONIQ_ADMIN_USER:-admin}"

  if [ -n "$CRONIQ_ADMIN_PASSWORD" ]; then
    ADMIN_PASS="$CRONIQ_ADMIN_PASSWORD"
    PASS_GENERATED=0
    if [ "$ADMIN_PASS" = "admin" ] && [ "${CRONIQ_DEMO_MODE:-0}" != "1" ]; then
      echo "ERROR: CRONIQ_ADMIN_PASSWORD='admin' is not allowed outside demo mode." >&2
      echo "       Set a strong password or add CRONIQ_DEMO_MODE=1 for local development." >&2
      exit 1
    fi
  else
    ADMIN_PASS="$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 24)"
    PASS_GENERATED=1
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
