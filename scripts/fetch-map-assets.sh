#!/usr/bin/env bash
set -euo pipefail

# Fetch the Breath of the Wild map tiles and marker data used by Wayfinder
# Slate. Assets come from zeldadungeon/maps at a pinned commit; only the BotW
# subtree is downloaded. The assets are third-party material, are not committed
# to this repository, and are verified against a SHA-256 manifest.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/frontend/static/map/botw"
MANIFEST="$ROOT/scripts/map-assets.sha256"
REPO="https://github.com/zeldadungeon/maps.git"
COMMIT="${WAYFINDER_MAPS_COMMIT:-d32a85656031d861cef38e32eb927a7d08a983a9}"

FORCE=0
VERIFY_ONLY=0
WRITE_MANIFEST=0

usage() {
  cat <<'EOF'
Usage: scripts/fetch-map-assets.sh [options]

Downloads the Breath of the Wild map tiles, icons, and marker JSON from
zeldadungeon/maps into frontend/static/map/botw.

Options:
  --force            Re-download even if the pinned commit is already present.
  --verify-only      Verify existing files against scripts/map-assets.sha256.
  --write-manifest   Regenerate scripts/map-assets.sha256 from the download.
  -h, --help         Show this help.

Environment:
  WAYFINDER_MAPS_COMMIT  Override the pinned upstream commit (testing only).
EOF
}

while (($# > 0)); do
  case "$1" in
    --force) FORCE=1 ;;
    --verify-only) VERIFY_ONLY=1 ;;
    --write-manifest) WRITE_MANIFEST=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

verify() {
  if [[ ! -f "$MANIFEST" ]]; then
    echo "error: checksum manifest not found at $MANIFEST" >&2
    echo "Re-run with --write-manifest after reviewing the downloaded assets." >&2
    exit 1
  fi
  if [[ ! -d "$DEST" ]]; then
    echo "error: map assets are not installed at $DEST" >&2
    exit 1
  fi
  echo "Verifying map assets against $(basename "$MANIFEST")..."
  (cd "$DEST" && sha256sum --quiet -c "$MANIFEST")
  echo "Map assets verified."
}

if ((VERIFY_ONLY)); then
  verify
  exit 0
fi

if [[ -f "$DEST/.source-commit" ]] && [[ "$(cat "$DEST/.source-commit")" == "$COMMIT" ]] && ((!FORCE)); then
  echo "Map assets for $COMMIT are already installed."
  verify
  exit 0
fi

for tool in git sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "error: $tool is required" >&2
    exit 1
  }
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Fetching zeldadungeon/maps at ${COMMIT:0:12} (BotW subtree only)..."
GIT_TERMINAL_PROMPT=0 git init --quiet "$TMP/repo"
GIT_TERMINAL_PROMPT=0 git -C "$TMP/repo" remote add origin "$REPO"
GIT_TERMINAL_PROMPT=0 git -C "$TMP/repo" fetch --quiet --depth 1 origin "$COMMIT"
git -C "$TMP/repo" sparse-checkout init --cone
git -C "$TMP/repo" sparse-checkout set public/botw
git -C "$TMP/repo" checkout --quiet FETCH_HEAD

if [[ ! -d "$TMP/repo/public/botw" ]]; then
  echo "error: upstream layout changed; public/botw was not found" >&2
  exit 1
fi

rm -rf "$DEST"
mkdir -p "$DEST"
cp -a "$TMP/repo/public/botw/." "$DEST/"
printf '%s\n' "$COMMIT" >"$DEST/.source-commit"

if ((WRITE_MANIFEST)); then
  echo "Writing manifest to $MANIFEST..."
  (
    cd "$DEST"
    find . -type f ! -name '.source-commit' -print0 |
      LC_ALL=C sort -z |
      xargs -0 sha256sum
  ) >"$MANIFEST"
fi

verify

FILES="$(find "$DEST" -type f | wc -l | tr -d ' ')"
SIZE="$(du -sh "$DEST" | cut -f1)"
echo "Installed $FILES BotW map files ($SIZE) into frontend/static/map/botw."
echo "Rebuild the frontend so rust-embed picks them up:"
echo "  npm --prefix frontend run build"
