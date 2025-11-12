#!/usr/bin/env bash
# open-profiles.sh — open VS Code windows for managed profiles
#
# Usage:
#   open-profiles.sh [--skip-install|-s] [--] [PROFILES...]
#
# Flags:
#   --skip-install, -s  Skip installing extensions for each profile.
#                       Same as setting env VSCODE_SKIP_EXTENSION_INSTALL=1.
#
# Environment:
#   VSCODE_SKIP_EXTENSION_INSTALL=1  Skip extension installation (useful for
#                                    first-run profile registration or theme preview).
#
# Notes:
# - When not skipping, the script installs extensions declared in
#   profiles/<name>/extensions.json using scripts/install-extensions.sh (cached by profile).
# - It also syncs the merged settings to VS Code’s profile cache when available
#   and opens a new Code window for each profile via `code --profile <name>`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILES_DIR="$ROOT/profiles"
INSTALL_SCRIPT="$ROOT/scripts/install-extensions.sh"
INSTALL_CACHE="$ROOT/.cache/extensions-installed"
SKIP_INSTALL="${VSCODE_SKIP_EXTENSION_INSTALL:-0}"
if [ ! -d "$PROFILES_DIR" ]; then
  echo "Profiles directory not found at $PROFILES_DIR" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "open-profiles: jq is required for profile cache syncing" >&2
  exit 1
fi

detect_code_user_dir() {
  if [ -n "${VSCODE_USER_DIR:-}" ]; then
    printf '%s\n' "$VSCODE_USER_DIR"
    return
  fi
  case "$(uname -s)" in
    Darwin)
      printf '%s\n' "$HOME/Library/Application Support/Code/User"
      ;;
    Linux)
      printf '%s\n' "${XDG_CONFIG_HOME:-$HOME/.config}/Code/User"
      ;;
    *)
      if [ -n "${APPDATA:-}" ]; then
        printf '%s\n' "$APPDATA/Code/User"
      else
        printf '%s\n' "$HOME/.config/Code/User"
      fi
      ;;
  esac
}

CODE_USER_DIR="$(detect_code_user_dir)"
PROFILE_STORAGE="$CODE_USER_DIR/globalStorage/storage.json"

if ! command -v code >/dev/null 2>&1; then
  echo "VS Code CLI 'code' not found in PATH" >&2
  exit 1
fi

mkdir -p "$INSTALL_CACHE"

# Parse CLI args: allow --skip-install flag and profiles list
ARGS=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --skip-install|-s)
      SKIP_INSTALL=1
      shift
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "open-profiles: unknown option $1" >&2
      exit 1
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done

# Any remaining args after -- are treated as profiles
if [ "$#" -gt 0 ]; then
  while [ "$#" -gt 0 ]; do ARGS+=("$1"); shift; done
fi

if [ "${#ARGS[@]}" -gt 0 ]; then
  mapfile -t PROFILES < <(printf '%s\n' "${ARGS[@]}")
else
  mapfile -t PROFILES < <(find "$PROFILES_DIR" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort)
fi

FILTERED=()
for name in "${PROFILES[@]}"; do
  if [ -d "$PROFILES_DIR/$name" ]; then
    FILTERED+=("$name")
  else
    echo "Warning: profile directory not found for '$name'" >&2
  fi
done

if [ "${#FILTERED[@]}" -eq 0 ]; then
  echo "No valid profiles to open." >&2
  exit 1
fi

PROFILES=("${FILTERED[@]}")

ensure_extensions() {
  local name="$1"
  local marker="$INSTALL_CACHE/$name"
  if [ "$SKIP_INSTALL" = "1" ]; then
    return 0
  fi
  if [ -f "$marker" ]; then
    return 0
  fi
  if [ -x "$INSTALL_SCRIPT" ]; then
    echo "Ensuring extensions for $name"
    if bash "$INSTALL_SCRIPT" "$name"; then
      touch "$marker"
    else
      echo "Warning: extension install failed for $name" >&2
    fi
  fi
}

sync_profile_cache() {
  local name="$1"
  local settings_src="$PROFILES_DIR/$name/settings.json"
  if [ ! -f "$settings_src" ]; then
    return 0
  fi
  if [ ! -f "$PROFILE_STORAGE" ]; then
    return 0
  fi
  local profile_id
  profile_id="$(jq -r --arg name "$name" '(.userDataProfiles // [])[] | select(.name==$name) | .location' "$PROFILE_STORAGE" 2>/dev/null || true)"
  if [ -z "$profile_id" ] || [ "$profile_id" = "null" ]; then
    echo "Warning: VS Code has no cached profile entry for $name; open it once manually to register." >&2
    return 0
  fi
  local target_dir="$CODE_USER_DIR/profiles/$profile_id"
  mkdir -p "$target_dir"
  if cp "$settings_src" "$target_dir/settings.json"; then
    echo "Synced $name -> $target_dir/settings.json"
  else
    echo "Warning: failed to sync cached settings for $name" >&2
  fi
}

for p in "${PROFILES[@]}"; do
  sync_profile_cache "$p"
  ensure_extensions "$p"
  echo "Opening profile: $p"
  code --profile "$p" --new-window || true
done

echo "All profiles opened. Use Profiles: Switch Profile in VS Code to confirm."
