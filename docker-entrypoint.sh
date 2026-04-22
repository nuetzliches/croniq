#!/bin/sh
set -e

DATA_DIR="${CRONIQ_DATA_DIR:-/var/lib/croniq}"
DB_FILE="$DATA_DIR/croniq.db"

# Auto-initialize on first run if DB doesn't exist
if [ ! -f "$DB_FILE" ]; then
  echo "First run detected — initializing database..."
  ADMIN_USER="${CRONIQ_ADMIN_USER:-admin}"

  if [ -n "$CRONIQ_ADMIN_PASSWORD" ]; then
    ADMIN_PASS="$CRONIQ_ADMIN_PASSWORD"
    PASS_GENERATED=0
  else
    ADMIN_PASS="$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 24)"
    PASS_GENERATED=1
  fi

  croniq init --data-dir "$DATA_DIR" --username "$ADMIN_USER" --password "$ADMIN_PASS"

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
