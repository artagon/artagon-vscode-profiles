#!/usr/bin/env bash
set -euo pipefail

if ! command -v jq >/dev/null 2>&1; then
  echo "export-profiles: jq is required but not found in PATH" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILES_DIR="$ROOT/profiles"
MERGED="$ROOT/_merged"
EXPORTS="$ROOT/exports"
mkdir -p "$EXPORTS"

if [ ! -d "$PROFILES_DIR" ]; then
  echo "export-profiles: profiles dir not found at $PROFILES_DIR" >&2
  exit 1
fi

mapfile -t PROFILES < <(find "$PROFILES_DIR" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort)

if [ "${#PROFILES[@]}" -eq 0 ]; then
  echo "export-profiles: no profiles to export" >&2
  exit 0
fi

for name in "${PROFILES[@]}"; do
  settings="$MERGED/$name.json"
  extensions="$PROFILES_DIR/$name/extensions.json"
  if [ ! -f "$settings" ]; then
    echo "export-profiles: skip $name (missing $settings). Run compose-settings.sh first." >&2
    continue
  fi
  if [ ! -f "$extensions" ]; then
    echo "export-profiles: skip $name (missing $extensions)" >&2
    continue
  fi
  jq -n \
    --slurpfile settings "$settings" \
    --slurpfile extensions "$extensions" \
    '
      {
        settings: $settings[0],
        extensions: {
          enabled: ($extensions[0] | map(.identifier.id))
        }
      }
    ' > "$EXPORTS/$name.code-profile"
  echo "Exported $name -> $EXPORTS/$name.code-profile"
done
