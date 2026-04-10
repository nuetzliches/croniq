#!/bin/sh
set -e

DATA_DIR="${CRONIQ_DATA_DIR:-/var/lib/croniq}"
DB_FILE="$DATA_DIR/croniq.db"

# Auto-initialize on first run if DB doesn't exist
if [ ! -f "$DB_FILE" ]; then
  echo "First run detected — initializing database..."
  ADMIN_USER="${CRONIQ_ADMIN_USER:-admin}"
  ADMIN_PASS="${CRONIQ_ADMIN_PASSWORD:-changeme}"
  croniq init --data-dir "$DATA_DIR" --username "$ADMIN_USER" --password "$ADMIN_PASS"
  echo ""
fi

exec "$@"
