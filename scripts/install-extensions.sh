#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=lib/extension-id.sh
source "$SCRIPT_DIR/lib/extension-id.sh"
# shellcheck source=lib/vsix-pin.sh
source "$SCRIPT_DIR/lib/vsix-pin.sh"
INSTALL_PINS_FILE="$(pins_file_path "$INSTALL_REPO_ROOT")"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for install-extensions.sh" >&2
  exit 1
fi

if ! command -v code >/dev/null 2>&1; then
  echo "VS Code CLI 'code' not found in PATH" >&2
  exit 1
fi

if [ "$#" -lt 1 ]; then
  echo "Usage: $(basename "$0") <profile> [--group <name> ...]" >&2
  echo "Use 'all' to install every tracked profile." >&2
  exit 1
fi

PROFILE=""
GROUP_FILTER=()
TARGET="profile"   # profile | global (workspace handled by install-workspace.sh)
DRY_RUN=0

normalize_group() {
  local g
  g="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
  case "$g" in
    ai) echo "AI" ;;
    cmake) echo "CMake" ;;
    java) echo "Java" ;;
    rust) echo "Rust" ;;
    general) echo "General" ;;
    *)
      echo ""
      ;;
  esac
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --target=*)
      TARGET="${1#*=}"
      case "$TARGET" in
        profile|global) ;;
        workspace) echo "Use install-workspace.sh for --target=workspace" >&2; exit 4 ;;
        *) echo "Unknown --target=$TARGET (valid: profile, global)" >&2; exit 4 ;;
      esac
      shift
      ;;
    --target)
      TARGET="$2"
      case "$TARGET" in
        profile|global) ;;
        workspace) echo "Use install-workspace.sh for --target=workspace" >&2; exit 4 ;;
        *) echo "Unknown --target $TARGET (valid: profile, global)" >&2; exit 4 ;;
      esac
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --group|--groups)
      if [ "$#" -lt 2 ]; then
        echo "Error: $1 requires a value" >&2
        exit 1
      fi
      IFS=',' read -ra groups <<<"$2"
      for raw in "${groups[@]}"; do
        norm="$(normalize_group "$raw")"
        if [ -z "$norm" ]; then
          echo "Warning: unknown group '$raw' (valid: AI, CMake, Java, Rust, General). Skipping." >&2
          continue
        fi
        GROUP_FILTER+=("$norm")
      done
      shift 2
      ;;
    *)
      if [ -z "$PROFILE" ]; then
        PROFILE="$1"
        shift
      else
        echo "Unknown argument: $1" >&2
        exit 1
      fi
      ;;
  esac
done

if [ -z "$PROFILE" ]; then
  echo "Error: profile is required" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Source legacy-name shim (per design.md Decision 21) so direct script
# callers also resolve `<flavor>-{crisp,retina}` names with deprecation
# warning. Best-effort: shim missing → behave as before.
if [ -f "$ROOT/scripts/lib/legacy-profile-name.sh" ]; then
  # shellcheck source=lib/legacy-profile-name.sh
  . "$ROOT/scripts/lib/legacy-profile-name.sh"
  resolved=$(resolve_legacy_profile_name "$PROFILE")
  if [ -n "$resolved" ]; then
    # resolved="<flavor> <ux>"; strip ux for install path (UX is workspace-applied)
    PROFILE="${resolved%% *}"
  fi
fi

GROUP_ARGS=()
if [ "${#GROUP_FILTER[@]}" -gt 0 ]; then
  for g in "${GROUP_FILTER[@]}"; do
    GROUP_ARGS+=("--group" "$g")
  done
fi

if [ "$PROFILE" = "all" ]; then
  ANY_FAILED=0
  for dir in "$ROOT"/profiles/*; do
    [ -d "$dir" ] || continue
    name="$(basename "$dir")"
    echo ">>> Installing extensions for $name"
    if [ "${#GROUP_ARGS[@]}" -gt 0 ]; then
      bash "$0" "$name" "${GROUP_ARGS[@]}" || ANY_FAILED=1
    else
      bash "$0" "$name" || ANY_FAILED=1
    fi
  done
  exit "$ANY_FAILED"
fi

EXT_FILE="$ROOT/profiles/$PROFILE/extensions.json"

if [ ! -f "$EXT_FILE" ]; then
  echo "Extensions file not found: $EXT_FILE" >&2
  exit 1
fi

EXT_IDS=()
# Materialize jq output first so a parse/syntax failure produces a non-zero
# exit instead of an empty pipe (which the loop would silently treat as
# "no extensions" and exit 0). Process substitution swallows jq's status.
EXT_LIST="$(jq -r '.[].identifier.id' "$EXT_FILE")" || {
  echo "Error: failed to parse $EXT_FILE with jq" >&2
  exit 1
}
while IFS= read -r ext; do
  [ -z "$ext" ] && continue
  if ! validate_extension_id "$ext"; then
    echo "Error: invalid extension id in $EXT_FILE; aborting before any install" >&2
    exit 1
  fi
  EXT_IDS+=("$ext")
done <<<"$EXT_LIST"

if [ "${#EXT_IDS[@]}" -eq 0 ]; then
  echo "No extensions listed in $EXT_FILE" >&2
  exit 0
fi

DELAY="${VSCODE_EXTENSION_INSTALL_DELAY:-1}"
FAILED_EXTS=()

should_install_group() {
  local group="$1"
  if [ "${#GROUP_FILTER[@]}" -eq 0 ]; then
    return 0
  fi
  for allowed in "${GROUP_FILTER[@]}"; do
    if [ "$allowed" = "$group" ]; then
      return 0
    fi
  done
  return 1
}

group_for_extension() {
  local ext="$1"
  case "$ext" in
    github.copilot|github.copilot-chat|anthropic.claude-code|openai.chatgpt|googlecloudtools.cloudcode|Continue.continue|continue.continue|codeium.codeium|tabnine.tabnine-vscode|sourcegraph.cody-ai|sourcegraph.amp)
      echo "AI"
      ;;
    ms-vscode.cmake-tools|twxs.cmake)
      echo "CMake"
      ;;
    redhat.java|redhat.vscode-xml|richardwillis.vscode-gradle|vscjava.vscode-java-pack|vscjava.vscode-java-test|vscjava.vscode-java-debug|vscjava.vscode-java-dependency|vscjava.vscode-maven|vscjava.vscode-gradle|gabrielbb.vscode-lombok|shengchen.vscode-checkstyle|pmd.pmd)
      echo "Java"
      ;;
    rust-lang.rust-analyzer|vadimcn.vscode-lldb|panicbit.cargo|serayuzgur.crates|tamasfe.even-better-toml|fill-labs.dependi)
      echo "Rust"
      ;;
    *)
      echo "General"
      ;;
  esac
}

declare -a ORDERED_GROUPS=("AI" "CMake" "Java" "Rust" "General")
declare -a GROUPED_EXTS=()

for ext in "${EXT_IDS[@]}"; do
  GROUPED_EXTS+=("$(group_for_extension "$ext")|$ext")
done

install_group() {
  local group="$1"
  local printed=0
  for entry in "${GROUPED_EXTS[@]}"; do
    IFS='|' read -r entry_group entry_ext <<<"$entry"
    if [ "$entry_group" != "$group" ]; then
      continue
    fi
    if ! should_install_group "$group"; then
      continue
    fi
    if [ "$printed" -eq 0 ]; then
      printf '\n=== %s extensions ===\n' "$group"
      printed=1
    fi

    if [ "$DRY_RUN" -eq 1 ]; then
      if [ "$TARGET" = "global" ]; then
        echo "DRY-RUN: would install $entry_ext globally (no --profile flag)"
      else
        echo "DRY-RUN: would install $entry_ext into profile $PROFILE"
      fi
      continue
    fi

    install_target="$entry_ext"
    if ! bypass_pins_active; then
      pin_data="$(lookup_pin "$entry_ext" "$INSTALL_PINS_FILE")"
      if [ -n "$pin_data" ]; then
        pin_sha256="$(printf '%s' "$pin_data" | sed -n '2p')"
        pin_url="$(printf '%s' "$pin_data" | sed -n '3p')"
        : "${VSIX_CACHE_DIR:=$(mktemp -d -t vsix-pin-XXXXXX)}"
        # Lazy trap install — only when we actually create the cache dir.
        if [ "${_vsix_cache_trapped:-0}" != "1" ]; then
          # shellcheck disable=SC2064
          trap "rm -rf '$VSIX_CACHE_DIR'" EXIT
          _vsix_cache_trapped=1
        fi
        vsix_path="$VSIX_CACHE_DIR/${entry_ext}.vsix"
        if fetch_and_verify_vsix "$entry_ext" "$pin_url" "$pin_sha256" "$vsix_path"; then
          install_target="$vsix_path"
        else
          FAILED_EXTS+=("$entry_ext")
          sleep "$DELAY"
          continue
        fi
      fi
    fi
    if [ "$TARGET" = "global" ]; then
      echo "Installing $entry_ext globally"
      if ! code --install-extension "$install_target" >/dev/null; then
        echo "Warning: failed to install $entry_ext" >&2
        FAILED_EXTS+=("$entry_ext")
      fi
    else
      echo "Installing $entry_ext for profile $PROFILE"
      if ! code --profile "$PROFILE" --install-extension "$install_target" >/dev/null; then
        echo "Warning: failed to install $entry_ext" >&2
        FAILED_EXTS+=("$entry_ext")
      fi
    fi
    sleep "$DELAY"
  done
}

for group in "${ORDERED_GROUPS[@]}"; do
  install_group "$group"
done

if [ "${#FAILED_EXTS[@]}" -gt 0 ]; then
  printf '\nThe following extensions failed to install for %s:\n' "$PROFILE"
  for ext in "${FAILED_EXTS[@]}"; do
    printf '  - %s\n' "$ext"
  done
  exit 1
fi

echo ""
echo "Finished installing extensions for $PROFILE"
