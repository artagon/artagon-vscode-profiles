#!/usr/bin/env bash
# compose-extensions.sh — compose layered extensions: base + toolchain(s).
# Implements `install-profile-extensions` "Layered extension composition".
#
# Usage:
#   compose_extensions <flavor> [<flavor>...]
# Outputs JSON array of {identifier: {id: <id>}} objects on stdout,
# deduped (first occurrence wins), order: base then each flavor in argument order.
# Exits non-zero if any flavor's layer file is missing.

# shellcheck disable=SC2148

compose_extensions() {
  local root
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  local layers_dir="$root/_shared/extensions"
  local files=("$layers_dir/base.json")

  local flavor
  for flavor in "$@"; do
    local f="$layers_dir/$flavor.json"
    if [[ ! -f "$f" ]]; then
      echo "error: missing layer file for flavor '$flavor': $f" >&2
      return 1
    fi
    files+=("$f")
  done

  if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq required" >&2
    return 1
  fi

  # Concatenate, dedup by .identifier.id (first wins), preserve order.
  jq -s 'add | unique_by(.identifier.id) as $deduped
        | reduce .[] as $item ([]; if any(.identifier.id == $item.identifier.id) then . else . + [$item] end)' \
    "${files[@]}"
}

# If sourced, expose function. If executed, run it directly.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  compose_extensions "$@"
fi
