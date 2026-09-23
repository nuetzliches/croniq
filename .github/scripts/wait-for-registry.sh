#!/usr/bin/env bash
# Wait until a just-published package version is actually served by its
# registry, and fail if it never shows up.
#
#   wait-for-registry.sh <label> <timeout-seconds> <url> [<url>...]
#
# Every URL must answer 2xx. Each one should be the registry's own document
# for that exact version (not a search index), so a 200 means `install` works.
#
# Used by the SDK release workflows (#784). An upload step exiting 0 is not
# the same as a package being installable: nuget.org validates after the push
# returns, Maven Central syncs to repo1 after the portal accepts the bundle,
# and proxy.golang.org only knows a tag once it has fetched it. Polling covers
# that lag; a URL still missing at the deadline fails the job. That job only
# waits, so if a registry was merely slow, "Re-run failed jobs" repeats the
# check without publishing anything again.

set -euo pipefail

if [ "$#" -lt 3 ]; then
  echo "usage: $0 <label> <timeout-seconds> <url>..." >&2
  exit 2
fi

label="$1"
timeout="$2"
shift 2
pending=("$@")
interval=20
deadline=$(( $(date +%s) + timeout ))

while :; do
  still=()
  for url in "${pending[@]}"; do
    # no-cache: npm and PyPI sit behind CDNs that may have cached the 404
    # from before the upload.
    if curl -fsS -o /dev/null -H 'Cache-Control: no-cache' "$url" 2>/dev/null; then
      echo "✓ $url"
    else
      still+=("$url")
    fi
  done
  if [ "${#still[@]}" -eq 0 ]; then
    echo "$label: every version is live on the registry"
    exit 0
  fi
  pending=("${still[@]}")
  if [ "$(date +%s)" -ge "$deadline" ]; then
    for url in "${pending[@]}"; do
      echo "::error::$label: not served after ${timeout}s: $url"
    done
    echo "The upload reported success but the registry does not serve the version." >&2
    echo "If the registry is only slow, re-run this job; it does not publish again." >&2
    exit 1
  fi
  left=$(( deadline - $(date +%s) ))
  nap=$(( left < interval ? left : interval ))
  echo "waiting ${nap}s for ${#pending[@]} URL(s)…"
  sleep "$nap"
done
