#!/usr/bin/env bash
# CI gate: assert UX presets carry identical key sets and don't overlap with editor-base.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CRISP="$ROOT/_shared/ux/crisp.jsonc"
RETINA="$ROOT/_shared/ux/retina.jsonc"
BASE="$ROOT/_shared/editor-base.jsonc"

for f in "$CRISP" "$RETINA" "$BASE"; do
  [[ -f "$f" ]] || { echo "missing: $f" >&2; exit 1; }
done

# Strip leading-line comments (the only comment form in our generated files);
# jq ingests the result as plain JSON.
strip_comments() {
  sed -E '/^[[:space:]]*\/\//d' "$1"
}

crisp_keys=$(strip_comments "$CRISP" | jq -S 'keys' -)
retina_keys=$(strip_comments "$RETINA" | jq -S 'keys' -)
base_keys=$(strip_comments "$BASE" | jq -S 'keys' -)

if [[ "$crisp_keys" != "$retina_keys" ]]; then
  echo "ERROR: crisp.jsonc and retina.jsonc have different key sets" >&2
  diff <(echo "$crisp_keys") <(echo "$retina_keys") >&2 || true
  exit 1
fi

overlap=$(jq -S -n --argjson a "$crisp_keys" --argjson b "$base_keys" '$a - ($a - $b)')
if [[ "$overlap" != "[]" ]]; then
  echo "ERROR: keys appear in both UX preset and editor-base:" >&2
  echo "$overlap" >&2
  exit 1
fi

echo "ok: UX symmetry verified"
echo "  ux keys: $(echo "$crisp_keys" | jq 'length') (identical between crisp and retina)"
echo "  base keys: $(echo "$base_keys" | jq 'length')"
