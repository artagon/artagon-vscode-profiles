#!/usr/bin/env bash
set -euo pipefail

if ! command -v jq >/dev/null 2>&1; then
  echo "validate-json: jq is required but not found in PATH" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SHARED="$ROOT/_shared"
OVR="$ROOT/_overrides"
PROFILES_DIR="$ROOT/profiles"

mapfile -t FILES < <(
  {
    find "$SHARED" -type f -name '*.jsonc' 2>/dev/null
    find "$OVR" -type f -name '*.jsonc' 2>/dev/null
    find "$PROFILES_DIR" -type f -name 'extensions.json' 2>/dev/null
  } | sort
)

if [ "${#FILES[@]}" -eq 0 ]; then
  echo "validate-json: no JSON/JSONC files found to validate" >&2
  exit 0
fi

failed=0
for file in "${FILES[@]}"; do
  if ! jq . "$file" >/dev/null 2>"$TMPDIR/validate-json.err.$$"; then
    echo "validate-json: FAILED: $file" >&2
    cat "$TMPDIR/validate-json.err.$$" >&2 || true
    failed=1
  fi
done
rm -f "$TMPDIR/validate-json.err.$$" 2>/dev/null || true

if [ "$failed" -ne 0 ]; then
  echo "validate-json: one or more files failed validation" >&2
  exit 1
fi

echo "validate-json: validated ${#FILES[@]} files successfully"
