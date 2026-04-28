#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/extension-id.sh
source "$SCRIPT_DIR/lib/extension-id.sh"

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
    echo "Installing $entry_ext for profile $PROFILE"
    if ! code --profile "$PROFILE" --install-extension "$entry_ext" >/dev/null; then
      echo "Warning: failed to install $entry_ext" >&2
      FAILED_EXTS+=("$entry_ext")
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
