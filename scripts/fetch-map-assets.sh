#!/usr/bin/env bash
# Thin Linux/macOS wrapper for the cross-platform Node asset fetcher.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v node >/dev/null 2>&1; then
  echo "error: Node.js 20+ is required to fetch map assets" >&2
  exit 1
fi

exec node "$ROOT/scripts/fetch-map-assets.mjs" "$@"
