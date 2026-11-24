#!/usr/bin/env bash
# apply-rust-policy.sh — write/update the Documentation Policy block
# between sentinels in agent docs. Idempotent on re-run.
#
# Reads §12 of docs/rust.md via extract-rust-policy.sh, then for each
# target file: replaces the existing sentinel-bracketed block, or
# appends a new one if no sentinels are present. Creates the file if
# missing.

set -euo pipefail

SCRIPT_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB="${SCRIPT_ROOT}/scripts/lib"

if [[ "${1:-}" == "--check" ]]; then
  CHECK_ONLY=1
  shift
else
  CHECK_ONLY=0
fi

SOURCE="$ROOT/docs/rust.md"
TARGETS=("$ROOT/AGENTS.md" "$ROOT/CLAUDE.md" "$ROOT/CODEX.md" "$ROOT/GEMINI.md")

POLICY=$("${BASH}" "$LIB/extract-rust-policy.sh" "$SOURCE")
[[ -n "$POLICY" ]] || { echo "error: empty policy extracted" >&2; exit 1; }

BLOCK=$(printf '<!-- BEGIN RUST-DOCS-POLICY -->\n%s\n<!-- END RUST-DOCS-POLICY -->' "$POLICY")

inspect_markers() {
  local file="${1}"
  awk '
    BEGIN {
      b_mark = "<!-- BEGIN RUST-DOCS-POLICY -->"
      e_mark = "<!-- END RUST-DOCS-POLICY -->"
      b_raw_tok = "BEGIN RUST-DOCS-POLICY"
      e_raw_tok = "END RUST-DOCS-POLICY"
      b_cnt = 0; e_cnt = 0; b_raw = 0; e_raw = 0
      b_line = 0; e_line = 0
    }
    {
      s = $0
      while ((idx = index(s, b_mark)) > 0) {
        b_cnt++
        if (b_line == 0) b_line = NR
        s = substr(s, idx + length(b_mark))
      }
      s = $0
      while ((idx = index(s, e_mark)) > 0) {
        e_cnt++
        if (e_line == 0) e_line = NR
        s = substr(s, idx + length(e_mark))
      }
      s = $0
      while ((idx = index(s, b_raw_tok)) > 0) {
        b_raw++
        s = substr(s, idx + length(b_raw_tok))
      }
      s = $0
      while ((idx = index(s, e_raw_tok)) > 0) {
        e_raw++
        s = substr(s, idx + length(e_raw_tok))
      }
    }
    END {
      if (b_raw != b_cnt || e_raw != e_cnt) {
        printf "malformed corrupted RUST-DOCS-POLICY sentinel (exact begin=%d end=%d, raw begin=%d end=%d)\n", b_cnt, e_cnt, b_raw, e_raw
      } else if (b_cnt == 0 && e_cnt == 0) {
        print "none"
      } else if (b_cnt == 1 && e_cnt == 1 && b_line < e_line) {
        print "valid"
      } else if (b_cnt > 0 && e_cnt == 0) {
        printf "malformed unmatched BEGIN marker (begin=%d, end=0)\n", b_cnt
      } else if (b_cnt == 0 && e_cnt > 0) {
        printf "malformed unmatched END marker (begin=0, end=%d)\n", e_cnt
      } else if (b_cnt != 1 || e_cnt != 1) {
        printf "malformed multiple policy markers (begin=%d, end=%d)\n", b_cnt, e_cnt
      } else {
        printf "malformed marker order (begin line %d >= end line %d)\n", b_line, e_line
      }
    }
  ' "${file}"
}

# Pre-validate all existing target files before rewriting any file so malformed
# markers never partially update targets or drop trailing file content.
if [[ $CHECK_ONLY -eq 0 ]]; then
  preflight_err=0
  for target in "${TARGETS[@]}"; do
    [[ -f "$target" ]] || continue
    marker_state="$(inspect_markers "$target")"
    case "$marker_state" in
      none|valid) ;;
      *)
        printf 'error: %s: %s; refusing to rewrite\n' "$target" "$marker_state" >&2
        preflight_err=1
        ;;
    esac
  done
  if [[ $preflight_err -ne 0 ]]; then
    exit 1
  fi
fi

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

  marker_state="$(inspect_markers "$target")"
  case "$marker_state" in
    valid)
      # Exactly one properly ordered BEGIN/END pair — extract current block, compare.
      current=$(awk '/<!-- BEGIN RUST-DOCS-POLICY -->/,/<!-- END RUST-DOCS-POLICY -->/' "$target")
      if [[ "$current" == "$BLOCK" ]]; then
        continue
      fi
      if [[ $CHECK_ONLY -eq 1 ]]; then
        echo "drift: $target policy block out of sync with docs/rust.md §12" >&2
        drift=1
        continue
      fi
      # Replace using temp block file so BSD macOS awk handles multiline content safely.
      block_tmp="$(mktemp "${TMPDIR:-/tmp}/rust-policy.XXXXXX")"
      out_tmp="${target}.tmp.$$"
      printf '%s\n' "$BLOCK" > "$block_tmp"
      if ! awk -v bfile="$block_tmp" '
        /<!-- BEGIN RUST-DOCS-POLICY -->/ {
          in_block=1
          while ((getline bline < bfile) > 0) print bline
          close(bfile)
          next
        }
        /<!-- END RUST-DOCS-POLICY -->/ { in_block=0; next }
        !in_block { print }
      ' "$target" > "$out_tmp"; then
        rm -f "$block_tmp" "$out_tmp"
        exit 1
      fi
      rm -f "$block_tmp"
      mv -f "$out_tmp" "$target"
      ;;
    none)
      if [[ $CHECK_ONLY -eq 1 ]]; then
        echo "drift: $target lacks RUST-DOCS-POLICY sentinels" >&2
        drift=1
        continue
      fi
      {
        printf '\n'
        printf '%s\n' "$BLOCK"
      } >> "$target"
      ;;
    *)
      printf 'error: %s: %s\n' "$target" "$marker_state" >&2
      if [[ $CHECK_ONLY -eq 1 ]]; then
        drift=1
        continue
      fi
      exit 1
      ;;
  esac
done

if [[ $CHECK_ONLY -eq 1 ]]; then
  if [[ $drift -eq 1 ]]; then
    echo "rust-docs policy drift detected; run 'bash scripts/lib/apply-rust-policy.sh' to sync" >&2
    exit 1
  fi
  echo "ok: rust-docs policy in sync across $(printf '%s\n' "${TARGETS[@]}" | wc -l) agent docs"
fi
