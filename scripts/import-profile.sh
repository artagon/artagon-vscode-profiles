#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
import-profile.sh - import a VS Code .code-profile bundle into a named profile

Usage:
  $(basename "$0") <profile-name> <path-to-code-profile>

Notes:
  - Requires jq and the VS Code 'code' CLI on PATH.
  - Creates/updates VS Code's cached profile entry under the user data dir
    (override with VSCODE_USER_DIR if you keep VS Code elsewhere).
  - Installs every extension listed in the bundle scoped to the profile.
USAGE
}

if [ "$#" -lt 2 ]; then
  usage >&2
  exit 1
fi

PROFILE="$1"
BUNDLE="$2"

if [ ! -f "$BUNDLE" ]; then
  echo "code-profile file not found: $BUNDLE" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "import-profile: jq is required" >&2
  exit 1
fi

if ! command -v code >/dev/null 2>&1; then
  echo "import-profile: VS Code CLI 'code' not found in PATH" >&2
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
mkdir -p "$(dirname "$PROFILE_STORAGE")"

if [ ! -f "$PROFILE_STORAGE" ]; then
  echo '{}' > "$PROFILE_STORAGE"
fi

existing_id="$(jq -r --arg name "$PROFILE" '(.userDataProfiles // [])[] | select(.name==$name) | .location' "$PROFILE_STORAGE" 2>/dev/null || true)"
if [ -n "$existing_id" ] && [ "$existing_id" != "null" ]; then
  PROFILE_ID="$existing_id"
else
  PROFILE_ID="$(LC_ALL=C tr -dc 'a-f0-9' < /dev/urandom | head -c 8)"
  tmp="$(mktemp)"
  jq --arg name "$PROFILE" --arg loc "$PROFILE_ID" '
    .userDataProfiles = ((.userDataProfiles // []) | map(select(.name != $name)) + [{name:$name, location:$loc}])
  ' "$PROFILE_STORAGE" > "$tmp"
  mv "$tmp" "$PROFILE_STORAGE"
fi

TARGET_DIR="$CODE_USER_DIR/profiles/$PROFILE_ID"
mkdir -p "$TARGET_DIR"

tmp_settings="$(mktemp)"
jq '.settings // {}' "$BUNDLE" > "$tmp_settings"
mv "$tmp_settings" "$TARGET_DIR/settings.json"
echo "Imported settings into profile '$PROFILE' (cache dir: $TARGET_DIR)"

mapfile -t EXTENSIONS < <(jq -r '.extensions.enabled[]?' "$BUNDLE" 2>/dev/null || true)
if [ "${#EXTENSIONS[@]}" -gt 0 ]; then
  DELAY="${VSCODE_EXTENSION_INSTALL_DELAY:-1}"
  for ext in "${EXTENSIONS[@]}"; do
    echo "Installing extension $ext for profile $PROFILE"
    code --profile "$PROFILE" --install-extension "$ext" >/dev/null || true
    sleep "$DELAY"
  done
fi

echo "Profile '$PROFILE' imported. Launch with: code --profile \"$PROFILE\" <folder>"
