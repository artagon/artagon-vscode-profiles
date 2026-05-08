#!/usr/bin/env bash
# apply-rust-policy.sh — write/update the Documentation Policy block
# between sentinels in agent docs. Idempotent on re-run.
#
# Reads §12 of docs/rust.md via extract-rust-policy.sh, then for each
# target file: replaces the existing sentinel-bracketed block, or
# appends a new one if no sentinels are present. Creates the file if
# missing.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIB="$ROOT/scripts/lib"

if [[ "${1:-}" == "--check" ]]; then
  CHECK_ONLY=1
  shift
else
  CHECK_ONLY=0
fi

SOURCE="$ROOT/docs/rust.md"
TARGETS=("$ROOT/AGENTS.md" "$ROOT/CLAUDE.md" "$ROOT/CODEX.md" "$ROOT/GEMINI.md")

POLICY=$(bash "$LIB/extract-rust-policy.sh" "$SOURCE")
[[ -n "$POLICY" ]] || { echo "error: empty policy extracted" >&2; exit 1; }

BLOCK=$(printf '<!-- BEGIN RUST-DOCS-POLICY -->\n%s\n<!-- END RUST-DOCS-POLICY -->' "$POLICY")

drift=0
for target in "${TARGETS[@]}"; do
  if [[ ! -f "$target" ]]; then
    if [[ $CHECK_ONLY -eq 1 ]]; then
      echo "drift: $target missing" >&2
      drift=1
      continue
    fi
    printf '%s\n' "$BLOCK" > "$target"
    continue
  fi

  if grep -q 'BEGIN RUST-DOCS-POLICY' "$target"; then
    # Sentinels present — extract current block, compare.
    current=$(awk '/<!-- BEGIN RUST-DOCS-POLICY -->/,/<!-- END RUST-DOCS-POLICY -->/' "$target")
    if [[ "$current" == "$BLOCK" ]]; then
      continue
    fi
    if [[ $CHECK_ONLY -eq 1 ]]; then
      echo "drift: $target policy block out of sync with docs/rust.md §12" >&2
      drift=1
      continue
    fi
    # Replace.
    awk -v block="$BLOCK" '
      /<!-- BEGIN RUST-DOCS-POLICY -->/ { in_block=1; print block; next }
      /<!-- END RUST-DOCS-POLICY -->/ { in_block=0; next }
      !in_block { print }
    ' "$target" > "$target.tmp"
    mv -f "$target.tmp" "$target"
  else
    if [[ $CHECK_ONLY -eq 1 ]]; then
      echo "drift: $target lacks RUST-DOCS-POLICY sentinels" >&2
      drift=1
      continue
    fi
    {
      printf '\n'
      printf '%s\n' "$BLOCK"
    } >> "$target"
  fi
done

if [[ $CHECK_ONLY -eq 1 ]]; then
  if [[ $drift -eq 1 ]]; then
    echo "rust-docs policy drift detected; run 'bash scripts/lib/apply-rust-policy.sh' to sync" >&2
    exit 1
  fi
  echo "ok: rust-docs policy in sync across $(printf '%s\n' "${TARGETS[@]}" | wc -l) agent docs"
fi
