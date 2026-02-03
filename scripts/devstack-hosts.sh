#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: devstack-hosts.sh [options]

Options:
  --domain <domain>       Override domain (default: CRONIQ_CADDY_DOMAIN or croniq.local)
  --hosts <list>          Space/comma separated host names (default: api dmz hooks ui)
  --ip <address>          IP address to map (default: 127.0.0.1)
  --hosts-file <path>     Hosts file path (default: /etc/hosts)
  --no-backup             Skip writing a .bak file
  -h, --help              Show this help
EOF
}

DOMAIN=""
HOSTS_RAW=""
IP_ADDRESS="127.0.0.1"
HOSTS_FILE="/etc/hosts"
NO_BACKUP="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --domain)
      DOMAIN="$2"
      shift 2
      ;;
    --hosts)
      HOSTS_RAW="$2"
      shift 2
      ;;
    --ip)
      IP_ADDRESS="$2"
      shift 2
      ;;
    --hosts-file)
      HOSTS_FILE="$2"
      shift 2
      ;;
    --no-backup)
      NO_BACKUP="true"
      shift 1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dotenv_path="$repo_root/.env"

get_dotenv_value() {
  local key="$1"
  [[ -f "$dotenv_path" ]] || return 1
  local line
  line=$(grep -E "^[[:space:]]*(export[[:space:]]+)?${key}=" "$dotenv_path" | tail -n 1 || true)
  [[ -n "$line" ]] || return 1
  local value="${line#*=}"
  value="${value%$'\r'}"
  value="${value#\"}"; value="${value%\"}"
  value="${value#\'}"; value="${value%\'}"
  printf '%s' "$value"
}

get_env_or_dotenv() {
  local key="$1"
  local value="${!key:-}"
  if [[ -n "$value" ]]; then
    printf '%s' "$value"
    return 0
  fi
  get_dotenv_value "$key"
}

if [[ -z "${DOMAIN// }" ]]; then
  DOMAIN=$(get_env_or_dotenv "CRONIQ_CADDY_DOMAIN" || true)
fi

if [[ -z "${DOMAIN// }" ]]; then
  DOMAIN="croniq.local"
fi

DOMAIN="${DOMAIN#.}"
DOMAIN="${DOMAIN%.}"

HOSTS=("api" "dmz" "hooks" "ui")
if [[ -n "${HOSTS_RAW// }" ]]; then
  HOSTS_RAW="${HOSTS_RAW//,/ }"
  read -r -a HOSTS <<< "$HOSTS_RAW"
fi

if [[ ${#HOSTS[@]} -eq 0 ]]; then
  echo "No hosts provided. Supply --hosts or leave the defaults." >&2
  exit 1
fi

if [[ ! -f "$HOSTS_FILE" ]]; then
  echo "Hosts file not found at $HOSTS_FILE" >&2
  exit 1
fi

if [[ ! -w "$HOSTS_FILE" ]]; then
  echo "Hosts file is not writable. Re-run with sudo." >&2
  exit 1
fi

resolved_hosts=()
declare -A seen_hosts
for host in "${HOSTS[@]}"; do
  trimmed="${host// }"
  [[ -n "$trimmed" ]] || continue
  if [[ "$trimmed" == *.* ]]; then
    resolved="$trimmed"
  else
    resolved="$trimmed.$DOMAIN"
  fi
  resolved=$(printf '%s' "$resolved" | tr '[:upper:]' '[:lower:]')
  if [[ -z "${seen_hosts[$resolved]:-}" ]]; then
    seen_hosts[$resolved]=1
    resolved_hosts+=("$resolved")
  fi
done

if [[ ${#resolved_hosts[@]} -eq 0 ]]; then
  echo "No hosts resolved. Supply --hosts or leave the defaults." >&2
  exit 1
fi

begin_marker="# croniq-devstack hosts (begin)"
end_marker="# croniq-devstack hosts (end)"
mapping_line="$IP_ADDRESS ${resolved_hosts[*]}"
block_lines=("$begin_marker" "$mapping_line" "$end_marker")

mapfile -t lines < "$HOSTS_FILE"
begin_index=-1
end_index=-1
for i in "${!lines[@]}"; do
  if [[ "${lines[$i]}" == "$begin_marker" ]]; then
    begin_index=$i
  elif [[ "${lines[$i]}" == "$end_marker" ]]; then
    end_index=$i
  fi
done

updated_lines=()
if [[ $begin_index -ge 0 && $end_index -gt $begin_index ]]; then
  if [[ $begin_index -gt 0 ]]; then
    for ((i=0; i<begin_index; i++)); do
      updated_lines+=("${lines[$i]}")
    done
  fi
  updated_lines+=("${block_lines[@]}")
  if [[ $((end_index + 1)) -lt ${#lines[@]} ]]; then
    for ((i=end_index + 1; i<${#lines[@]}; i++)); do
      updated_lines+=("${lines[$i]}")
    done
  fi
else
  updated_lines+=("${lines[@]}")
  if [[ ${#updated_lines[@]} -gt 0 && "${updated_lines[-1]}" != "" ]]; then
    updated_lines+=("")
  fi
  updated_lines+=("${block_lines[@]}")
fi

if [[ "$NO_BACKUP" != "true" ]]; then
  cp "$HOSTS_FILE" "$HOSTS_FILE.bak"
  echo "[devstack] Backup written to $HOSTS_FILE.bak"
fi

printf "%s\n" "${updated_lines[@]}" > "$HOSTS_FILE"

echo "[devstack] Hosts updated: $mapping_line"
