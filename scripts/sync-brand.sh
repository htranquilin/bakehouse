#!/usr/bin/env bash
# Sync canonical brand assets (visual-identity/) into the app:
#   tokens.css -> src/styles/tokens.css   (checked in; re-run on brand changes)
#   app-icon.svg -> src-tauri/icons/      (via scripts/build-icons.mjs)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

cp "$ROOT/visual-identity/tokens.css" "$ROOT/src/styles/tokens.css"
node "$ROOT/scripts/build-icons.mjs"
echo "Brand synced."
