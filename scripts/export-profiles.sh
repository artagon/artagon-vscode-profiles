#!/usr/bin/env bash
set -euo pipefail

if ! command -v jq >/dev/null 2>&1; then
  echo "export-profiles: jq is required but not found in PATH" >&2
  exit 1
fi

## sha256_for_file <path>
## Prints the lowercase hex sha256 of <path> using sha256sum (Linux) or
## shasum -a 256 (macOS). Exits non-zero if neither is available.
sha256_for_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    echo "export-profiles: need sha256sum or shasum for SHA256SUMS write" >&2
    return 1
  fi
}

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILES_DIR="$ROOT/profiles"
MERGED="$ROOT/_merged"
EXPORTS="$ROOT/exports"
mkdir -p "$EXPORTS"

if [ ! -d "$PROFILES_DIR" ]; then
  echo "export-profiles: profiles dir not found at $PROFILES_DIR" >&2
  exit 1
fi

PROFILES=()
while IFS= read -r name; do
  [ -n "$name" ] && PROFILES+=("$name")
done < <(find "$PROFILES_DIR" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort)

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

## Write exports/SHA256SUMS — one line per non-symlink .code-profile,
## GNU coreutils format ("<sha256>  <basename>"), sorted by basename.
## import-profile.sh verifies bundles against this file when present.
SUMS_TMP="${EXPORTS}/SHA256SUMS.tmp.$$"
: > "${SUMS_TMP}"
while IFS= read -r bundle; do
  hash=$(sha256_for_file "${bundle}")
  printf '%s  %s\n' "${hash}" "$(basename "${bundle}")" >> "${SUMS_TMP}"
done < <(find "${EXPORTS}" -maxdepth 1 -type f -name '*.code-profile' | sort)

if [ -s "${SUMS_TMP}" ]; then
  sort -k2 "${SUMS_TMP}" -o "${SUMS_TMP}"
  mv -f "${SUMS_TMP}" "${EXPORTS}/SHA256SUMS"
  echo "Wrote ${EXPORTS}/SHA256SUMS"
else
  rm -f "${SUMS_TMP}"
fi

printf '%s\n' \
  'Import bundles via: scripts/import-profile.sh <name> exports/<name>.code-profile' \
  'NOTE: the VS Code UI import does NOT install missing extensions and may produce a degraded look.'
