#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILES_DIR="$ROOT/profiles"
SHARED="$ROOT/_shared"
OVR="$ROOT/_overrides"
MERGED="$ROOT/_merged"
mkdir -p "$MERGED" "$PROFILES_DIR"
# Collect override chain (parents first, then file), compatible with older bash and with cycle detection
collect_overrides() {
  local file="$1" stack="$2"
  [ -f "$file" ] || return 0
  case " $stack " in
    *" $file "*)
      echo "Error: detected @extends cycle: $stack -> $file" >&2
      return 1
      ;;
  esac
  local parents=()
  while IFS= read -r parent; do
    [ -n "$parent" ] && parents+=("$parent")
  done < <(jq -r '."@extends"? // empty | (if type=="string" then . else .[] end)' "$file" 2>/dev/null || true)
  local new_stack="$stack $file"
  if [ "${#parents[@]}" -gt 0 ]; then
    for parent in "${parents[@]}"; do
      local parent_path="$OVR/$parent"
      if [ -f "$parent_path" ]; then
        collect_overrides "$parent_path" "$new_stack" || return 1
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
  # Merge: base first, then override wins for conflicts
  jq -s 'reduce .[] as $it ({}; . * $it)' "${inputs[@]}" > "$MERGED/$name.json"
  rm -f "${temps[@]}"
  # Replace profile settings.json with repo-relative symlink to merged output
  local rel_target="../../_merged/$name.json"
  ln -sfn "$rel_target" "$profile_dir/settings.json"
  echo "Merged $name -> $MERGED/$name.json"
}

if [ "$#" -gt 0 ]; then
  for n in "$@"; do merge_one "$n"; done
else
  for d in "$PROFILES_DIR"/*; do
    [ -d "$d" ] || continue
    n="$(basename "$d")"
    merge_one "$n"
  done
fi
