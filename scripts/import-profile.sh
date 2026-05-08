#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=lib/extension-id.sh
source "$SCRIPT_DIR/lib/extension-id.sh"
# shellcheck source=lib/vsix-pin.sh
source "$SCRIPT_DIR/lib/vsix-pin.sh"

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

## sha256_for_file <path>
## Prints the lowercase hex sha256 of <path>. Mirrors the helper in
## export-profiles.sh; same hasher detection (sha256sum / shasum -a 256).
sha256_for_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    echo "import-profile: need sha256sum or shasum for SHA256SUMS verification" >&2
    return 1
  fi
}

## Verify the bundle against <bundle-dir>/SHA256SUMS when present.
## Behaviour:
##   - SHA256SUMS absent           -> warn-and-proceed.
##   - bundle not listed in file   -> warn-and-proceed.
##   - bundle listed, hash matches -> silent success.
##   - bundle listed, hash drifts  -> hard fail before any write.
BUNDLE_DIR="$(cd "$(dirname "$BUNDLE")" && pwd)"
BUNDLE_BASENAME="$(basename "$BUNDLE")"
SUMS_FILE="${BUNDLE_DIR}/SHA256SUMS"
if [ -f "${SUMS_FILE}" ]; then
  expected_hash="$(awk -v name="${BUNDLE_BASENAME}" '$2 == name { print $1; exit }' "${SUMS_FILE}")"
  if [ -z "${expected_hash}" ]; then
    echo "import-profile: ${BUNDLE_BASENAME} not listed in SHA256SUMS; integrity unverified" >&2
  else
    actual_hash="$(sha256_for_file "${BUNDLE}")"
    if [ "${expected_hash}" != "${actual_hash}" ]; then
      printf 'import-profile: SHA256 mismatch for %s\n  expected: %s\n  actual:   %s\n' \
        "${BUNDLE_BASENAME}" "${expected_hash}" "${actual_hash}" >&2
      exit 1
    fi
  fi
else
  echo "import-profile: SHA256SUMS not found in ${BUNDLE_DIR}; bundle integrity unverified" >&2
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "import-profile: jq is required" >&2
  exit 1
fi

if ! command -v code >/dev/null 2>&1; then
  echo "import-profile: VS Code CLI 'code' not found in PATH" >&2
  exit 1
fi

## resolve_theme_to_ext_id <value>
## Maps a workbench.{colorTheme,iconTheme} value to its backing
## <publisher>.<name> extension ID, by these rules (mirrors
## openspec/changes/profile-graceful-defaults/specs/import-profile-bundles/spec.md):
##   1. value already in <publisher>.<name> form -> echoed as-is.
##   2. value is a known human-readable label from this repo's themes ->
##      mapped via the case statement below.
##   3. otherwise -> empty stdout (caller leaves the key in place;
##      VS Code is presumed to resolve it as a built-in theme).
##
## Coverage: all theme/icon-theme labels referenced by this repo's
## _shared/editor-{crisp,retina}.jsonc and _shared/ux/{crisp,retina}.jsonc,
## plus the primary labels for every theme/icon-theme extension shipped in
## profiles/*/extensions.json. Add entries here when introducing a new
## theme to _shared/ or _overrides/.
##
## Bash 3.2 (macOS /bin/bash) lacks associative arrays, so this is a case
## statement. Reuses EXTENSION_ID_REGEX from lib/extension-id.sh so the
## explicit-ID form goes through the same gate as `code --install-extension`.
resolve_theme_to_ext_id() {
  local value="$1"
  if [[ "${value}" =~ $EXTENSION_ID_REGEX ]]; then
    printf '%s\n' "${value}"
    return 0
  fi
  case "${value}" in
    "Tokyo Night"|"Tokyo Night Storm"|"Tokyo Night Light")
      printf '%s\n' 'enkia.tokyo-night' ;;
    "Catppuccin Mocha"|"Catppuccin Macchiato"|"Catppuccin Frappé"|"Catppuccin Frappe"|"Catppuccin Latte")
      printf '%s\n' 'catppuccin.catppuccin-vsc' ;;
    "catppuccin-icons"|"catppuccin-perfect-mocha"|"catppuccin-perfect-macchiato"|"catppuccin-perfect-frappe"|"catppuccin-perfect-latte")
      printf '%s\n' 'catppuccin.catppuccin-vsc-icons' ;;
    "material-icon-theme")
      printf '%s\n' 'pkief.material-icon-theme' ;;
    "One Dark Pro"|"One Dark Pro Darker"|"One Dark Pro Flat"|"One Dark Pro Mix")
      printf '%s\n' 'zhuangtongfa.Material-theme' ;;
    "Night Owl"|"Night Owl Light")
      printf '%s\n' 'sdras.night-owl' ;;
    "Atom One Dark"|"Atom One Light")
      printf '%s\n' 'akamud.vscode-theme-onedark' ;;
    "Dracula"|"Dracula Soft")
      printf '%s\n' 'dracula-theme.theme-dracula' ;;
    "vscode-icons")
      printf '%s\n' 'vscode-icons-team.vscode-icons' ;;
  esac
}

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
  # PROFILE_ID needs a SIGPIPE-safe RNG; the previous tr | head -c idiom
  # aborted under set -euo pipefail. Try openssl first, then python3 — and
  # fall back through if either is *broken*, not just absent. The earlier
  # shape only checked `command -v`, so a present-but-degraded openssl
  # (FIPS rejecting `rand`, missing entropy source, etc.) would abort under
  # set -e before ever reaching python3.
  PROFILE_ID=""
  if command -v openssl >/dev/null 2>&1; then
    PROFILE_ID="$(openssl rand -hex 4 2>/dev/null || true)"
  fi
  if [ -z "$PROFILE_ID" ] && command -v python3 >/dev/null 2>&1; then
    PROFILE_ID="$(python3 -c 'import secrets; print(secrets.token_hex(4))' 2>/dev/null || true)"
  fi
  # Sanity-check shape — even a successful generator could in theory return
  # something other than 8 lowercase hex chars (truncated entropy, locale
  # weirdness, etc.). Keep the contract tight.
  if [[ ! "$PROFILE_ID" =~ ^[0-9a-f]{8}$ ]]; then
    echo "import-profile: needs working 'openssl rand' or python3 'secrets.token_hex' for PROFILE_ID generation; install or repair one and retry" >&2
    exit 1
  fi
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

EXTENSIONS=()
# Materialize jq output first; otherwise a malformed extensions.enabled
# (e.g. {"enabled": 1}) would silently fail inside the process substitution
# and the loop would treat it as "no extensions" while still completing
# settings import.
EXT_LIST="$(jq -r '.extensions.enabled[]?' "$BUNDLE")" || {
  echo "Error: failed to parse $BUNDLE with jq" >&2
  exit 1
}
while IFS= read -r ext; do
  [ -z "$ext" ] && continue
  if ! validate_extension_id "$ext"; then
    echo "Error: invalid extension id in $BUNDLE; aborting before any install" >&2
    exit 1
  fi
  EXTENSIONS+=("$ext")
done <<<"$EXT_LIST"
FAILED_EXT=()
PINS_FILE="$(pins_file_path "$REPO_ROOT")"
if [ "${#EXTENSIONS[@]}" -gt 0 ]; then
  DELAY="${VSCODE_EXTENSION_INSTALL_DELAY:-1}"
  VSIX_CACHE_DIR="$(mktemp -d -t vsix-pin-XXXXXX)"
  trap 'rm -rf "$VSIX_CACHE_DIR"' EXIT
  for ext in "${EXTENSIONS[@]}"; do
    echo "Installing extension $ext for profile $PROFILE"
    install_target="$ext"
    if ! bypass_pins_active; then
      pin_data="$(lookup_pin "$ext" "$PINS_FILE")"
      if [ -n "$pin_data" ]; then
        # Pin entry: 3 lines (version, sha256, vsix_url).
        pin_version="$(printf '%s' "$pin_data" | sed -n '1p')"
        pin_sha256="$(printf '%s' "$pin_data" | sed -n '2p')"
        pin_url="$(printf '%s' "$pin_data" | sed -n '3p')"
        vsix_path="$VSIX_CACHE_DIR/${ext}.vsix"
        if fetch_and_verify_vsix "$ext" "$pin_url" "$pin_sha256" "$vsix_path"; then
          echo "  -> verified pinned VSIX ${ext}@${pin_version}"
          install_target="$vsix_path"
        else
          # Fail-closed: do NOT fall back to live marketplace.
          FAILED_EXT+=("$ext")
          sleep "$DELAY"
          continue
        fi
      fi
    fi
    if ! code --profile "$PROFILE" --install-extension "$install_target" >/dev/null; then
      echo "Warning: failed to install $ext" >&2
      FAILED_EXT+=("$ext")
    fi
    sleep "$DELAY"
  done
fi
## rewrite_settings_for_failed_themes
## Strips workbench.colorTheme / workbench.iconTheme from the imported
## profile cache when the backing extension appears in FAILED_EXT.
## Atomic write via a sibling tempfile (matches compose-settings.sh:144-150).
## Caller MUST gate on `[ "${#FAILED_EXT[@]}" -gt 0 ]` — this function
## dereferences the array and would abort under bash 3.2 + set -u when
## the array is empty.
rewrite_settings_for_failed_themes() {
  local file="${TARGET_DIR}/settings.json"
  if [ ! -f "${file}" ]; then
    return 0
  fi

  local removed=()
  local key
  for key in "workbench.colorTheme" "workbench.iconTheme"; do
    local value
    value=$(jq -r --arg k "${key}" '.[$k] // empty' "${file}")
    if [ -z "${value}" ]; then
      continue
    fi
    local id
    id=$(resolve_theme_to_ext_id "${value}")
    if [ -z "${id}" ]; then
      continue
    fi
    local f
    for f in "${FAILED_EXT[@]}"; do
      if [ "${f}" = "${id}" ]; then
        removed+=("${key}")
        printf 'import-profile: removed %s (extension %s failed to install)\n' "${key}" "${id}" >&2
        break
      fi
    done
  done

  if [ "${#removed[@]}" -eq 0 ]; then
    return 0
  fi

  local jq_expr='.'
  local k
  for k in "${removed[@]}"; do
    jq_expr="${jq_expr} | del(.[\"${k}\"])"
  done

  local out_tmp="${TARGET_DIR}/settings.json.tmp.$$"
  if ! jq "${jq_expr}" "${file}" > "${out_tmp}"; then
    rm -f "${out_tmp}"
    printf 'import-profile: failed to rewrite %s; leaving original in place\n' "${file}" >&2
    return 1
  fi
  mv -f "${out_tmp}" "${file}"
}

if [ "${#FAILED_EXT[@]}" -gt 0 ]; then
  rewrite_settings_for_failed_themes
  printf '\nFailed installs for %s:\n' "$PROFILE" >&2
  for e in "${FAILED_EXT[@]}"; do printf '  - %s\n' "$e" >&2; done
  exit 1
fi

echo "Profile '$PROFILE' imported. Launch with: code --profile \"$PROFILE\" <folder>"
