#!/usr/bin/env bash
# jsonc-merge.sh — key-level merge JSON onto an existing JSONC file.
#
# Implements `compose-profile-settings` "Workspace UX overrides" merge:
#   1. Read existing JSONC file (comments stripped via jsonc-strip.awk).
#   2. Apply jq deep-merge with the new JSON document.
#   3. Write back canonical JSON with the original file's leading
#      comment block preserved as a string prefix.
#   4. Write a `.bak` sibling on first merge into a comment-bearing file.
#
# Usage:
#   jsonc_merge <target-path> <new-json-string-or-file>
# Where the second arg is either a JSON document on stdin (when "-") or
# a path to a JSON/JSONC file. The new doc's keys win on conflict.

# shellcheck disable=SC2148

jsonc_merge() {
  local target="$1"
  local new_doc_arg="$2"
  local lib_dir
  lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  local strip="$lib_dir/jsonc-strip.awk"

  if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq required" >&2
    return 1
  fi
  if [[ ! -f "$strip" ]]; then
    echo "error: missing $strip" >&2
    return 1
  fi

  local existing_json="{}"
  local leading_comment=""
  local had_interior_comments=0

  if [[ -f "$target" ]]; then
    # Capture leading comment block: every consecutive line starting with
    # whitespace-then-`//` from the top, up to the first non-comment.
    leading_comment=$(awk '
      /^[[:space:]]*\/\// { print; next }
      /^[[:space:]]*$/ && header_done == 0 { print; next }
      { header_done=1; exit }
    ' "$target")

    # Detect interior comments for backup decision.
    if grep -E '^[[:space:]]*[^/[:space:]].*//' "$target" >/dev/null 2>&1 \
       || grep -E '/\*' "$target" >/dev/null 2>&1; then
      had_interior_comments=1
    fi

    existing_json=$(awk -f "$strip" "$target") || {
      echo "error: failed to strip JSONC from $target" >&2
      return 1
    }
  fi

  local new_doc
  if [[ "$new_doc_arg" == "-" ]]; then
    new_doc=$(cat)
  elif [[ -f "$new_doc_arg" ]]; then
    new_doc=$(awk -f "$strip" "$new_doc_arg")
  else
    new_doc="$new_doc_arg"
  fi

  # Validate both inputs as JSON.
  if ! echo "$existing_json" | jq empty >/dev/null 2>&1; then
    echo "error: existing $target is not valid JSON after comment-strip" >&2
    return 1
  fi
  if ! echo "$new_doc" | jq empty >/dev/null 2>&1; then
    echo "error: new document is not valid JSON" >&2
    return 1
  fi

  # Deep merge: objects merge key-by-key, primitives/arrays replace.
  local merged
  merged=$(jq -s '.[0] * .[1]' <(echo "$existing_json") <(echo "$new_doc")) || {
    echo "error: jq merge failed" >&2
    return 1
  }

  # Backup if the file had interior comments (lossy merge).
  if [[ -f "$target" ]] && [[ $had_interior_comments -eq 1 ]] && [[ ! -f "$target.bak" ]]; then
    cp "$target" "$target.bak"
  fi

  # Atomic write via temp file in the same directory.
  local dir
  dir=$(dirname "$target")
  local tmp="$dir/.jsonc-merge.tmp.$$"

  {
    if [[ -n "$leading_comment" ]]; then
      printf '%s\n' "$leading_comment"
    fi
    echo "$merged" | jq .
  } > "$tmp"

  mv -f "$tmp" "$target"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  jsonc_merge "$@"
fi
