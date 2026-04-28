#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILES_DIR="$ROOT/profiles"
SHARED="$ROOT/_shared"
OVR="$ROOT/_overrides"
MERGED="$ROOT/_merged"
mkdir -p "$MERGED" "$PROFILES_DIR"

# Accepted shape for @extends values. Permits bare filenames or a SINGLE-LEVEL
# subpath (e.g. "ai/copilot.jsonc" — the only depth currently used in the repo).
# Rejects traversal (..), absolute paths (/), home (~), backslashes, NUL, multi-
# level subdirs, and anything else not made of [A-Za-z0-9._-] segments.
EXTENDS_NAME_RE='^[a-zA-Z0-9._-]+(/[a-zA-Z0-9._-]+)?\.jsonc$'

# realpath shim — macOS does not ship GNU realpath in stock; fall back to python.
resolve_real_path() {
  local p="$1"
  if command -v realpath >/dev/null 2>&1; then
    realpath "$p"
  else
    python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$p"
  fi
}

# Real-path of $OVR for containment checks.
OVR_REAL="$(resolve_real_path "$OVR")"

# Collect override chain (parents first, then file), compatible with older bash and with cycle
# detection. Cycles are detected by resolved real path so symlinked overrides cannot trick the
# detector. Ancestors are tracked in a global array (CYCLE_STACK) rather than a packed string
# so paths containing spaces compare correctly. Each call pushes its real path, recurses, then
# pops on return — depth-balanced via a single return path.
CYCLE_STACK=()
collect_overrides() {
  local file="$1"
  [ -f "$file" ] || return 0
  local real
  real="$(resolve_real_path "$file")"
  local i
  for i in "${CYCLE_STACK[@]+"${CYCLE_STACK[@]}"}"; do
    if [ "$i" = "$real" ]; then
      echo "Error: detected @extends cycle ending at: $real" >&2
      return 1
    fi
  done
  CYCLE_STACK+=("$real")
  local rc=0
  _collect_inner "$file" || rc=$?
  unset 'CYCLE_STACK[${#CYCLE_STACK[@]}-1]'
  return $rc
}

_collect_inner() {
  local file="$1"
  local parents=()
  # Strict @extends parsing — let jq errors propagate so malformed values surface loudly.
  while IFS= read -r parent; do
    [ -n "$parent" ] && parents+=("$parent")
  done < <(jq -r '."@extends"? // empty | (if type=="string" then . elif type=="array" then .[] else error("@extends must be string or array of strings") end)' "$file")
  if [ "${#parents[@]}" -gt 0 ]; then
    for parent in "${parents[@]}"; do
      # Lexical guard: reject anything not made of safe segments.
      if [[ ! "$parent" =~ $EXTENDS_NAME_RE ]]; then
        echo "Error: rejected @extends path '$parent' in $file (must match $EXTENDS_NAME_RE; no traversal, absolute paths, ~, backslashes, or NUL)" >&2
        return 1
      fi
      local parent_path="$OVR/$parent"
      # Containment guard: even safe-looking names must resolve inside _overrides/.
      if [ -e "$parent_path" ]; then
        local parent_real
        parent_real="$(resolve_real_path "$parent_path")"
        case "$parent_real" in
          "$OVR_REAL"/*) ;;
          *)
            echo "Error: @extends '$parent' in $file resolves outside _overrides/ ($parent_real)" >&2
            return 1
            ;;
        esac
        collect_overrides "$parent_path" || return 1
      else
        echo "Warning: missing extends file $parent referenced by $file" >&2
      fi
    done
  fi
  echo "$file"
}
merge_one() {
  local name="$1"
  local base="$SHARED/editor-crisp.jsonc"
  if [[ "$name" == *retina* ]]; then base="$SHARED/editor-retina.jsonc"; fi
  local override="$OVR/$name.jsonc"
  if [ ! -f "$override" ]; then
    echo "Skip $name (no override)" >&2
    return 0
  fi
  local profile_dir="$PROFILES_DIR/$name"
  if [ ! -d "$profile_dir" ]; then
    echo "Skip $name (profile dir missing at $profile_dir)" >&2
    return 0
  fi
  local overrides_output
  if ! overrides_output="$(collect_overrides "$override" "")"; then
    echo "Skip $name due to earlier errors" >&2
    return 1
  fi
  local overrides=()
  while IFS= read -r line; do
    [ -n "$line" ] && overrides+=("$line")
  done <<<"$overrides_output"
  if [ "${#overrides[@]}" -eq 0 ]; then
    echo "Skip $name (no resolvable overrides)" >&2
    return 0
  fi
  local -a inputs=("$base")
  local -a temps=()
  for o in "${overrides[@]}"; do
    local tmp
    tmp="$(mktemp)"
    jq 'del(."@extends")' "$o" > "$tmp"
    temps+=("$tmp")
    inputs+=("$tmp")
  done
  # Merge: base first, then override wins for conflicts.
  # Atomic write: produce content in a sibling temp file and rename so readers (VS Code via the
  # profile settings.json symlink) never observe a truncated or partial file.
  local out_tmp="$MERGED/$name.json.tmp.$$"
  if ! jq -s 'reduce .[] as $it ({}; . * $it)' "${inputs[@]}" > "$out_tmp"; then
    rm -f "$out_tmp" "${temps[@]}"
    echo "Error: jq merge failed for $name; existing $MERGED/$name.json preserved" >&2
    return 1
  fi
  mv -f "$out_tmp" "$MERGED/$name.json"
  rm -f "${temps[@]}"
  # Replace profile settings.json with repo-relative symlink to merged output
  local rel_target="../../_merged/$name.json"
  ln -sfn "$rel_target" "$profile_dir/settings.json"
  echo "Merged $name -> $MERGED/$name.json"
}

FAILED=()
if [ "$#" -gt 0 ]; then
  for n in "$@"; do
    if ! merge_one "$n"; then
      FAILED+=("$n")
    fi
  done
else
  for d in "$PROFILES_DIR"/*; do
    [ -d "$d" ] || continue
    n="$(basename "$d")"
    if ! merge_one "$n"; then
      FAILED+=("$n")
    fi
  done
fi

if [ "${#FAILED[@]}" -gt 0 ]; then
  printf '\ncompose-settings: failed for %d profile(s):\n' "${#FAILED[@]}" >&2
  for n in "${FAILED[@]}"; do printf '  - %s\n' "$n" >&2; done
  exit 1
fi
