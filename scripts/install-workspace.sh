#!/usr/bin/env bash
# install-workspace.sh — write `.vscode/*` files from composed toolchain
# layers. Implements `--target=workspace` per `install-profile-extensions`.
#
# Writes:
#   .vscode/extensions.json   (recommendations, merged with existing)
#   .vscode/settings.json     (editor-base + optional UX + rust hover +
#                              rtk profile, jsonc-merged)
#   .vscode/tasks.json        (rust doc tasks if rust in scope,
#                              overwrite-by-label)
#
# Usage:
#   install-workspace.sh <workspace> [OPTIONS] <toolchain> [<toolchain>...]
# Options:
#   --ux=PRESET          Apply named UX preset (crisp|retina).
#   --font=NAME          Override editor.fontFamily.
#   --font-size=N        Override editor.fontSize.
#   --theme=ID           Override workbench.colorTheme.
#   --icon-theme=ID      Override workbench.iconTheme.
#   --no-rtk             Skip rtk terminal profile emission.
#   --dry-run            Print would-be writes; modify nothing.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB="$ROOT/scripts/lib"

# shellcheck source=lib/compose-extensions.sh
source "$LIB/compose-extensions.sh"
# shellcheck source=lib/jsonc-merge.sh
source "$LIB/jsonc-merge.sh"

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# //; s/^#$//'
}

WORKSPACE=""
UX=""
FONT=""
FONT_SIZE=""
THEME=""
ICON_THEME=""
NO_RTK=0
DRY_RUN=0
CHECK_COMPAT="warn"   # block | warn | off (per design.md Decision 11)
TOOLCHAINS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ux=*)         UX="${1#*=}"; shift ;;
    --ux)           UX="$2"; shift 2 ;;
    --font=*)       FONT="${1#*=}"; shift ;;
    --font)         FONT="$2"; shift 2 ;;
    --font-size=*)  FONT_SIZE="${1#*=}"; shift ;;
    --font-size)    FONT_SIZE="$2"; shift 2 ;;
    --theme=*)      THEME="${1#*=}"; shift ;;
    --theme)        THEME="$2"; shift 2 ;;
    --icon-theme=*) ICON_THEME="${1#*=}"; shift ;;
    --icon-theme)   ICON_THEME="$2"; shift 2 ;;
    --no-rtk)       NO_RTK=1; shift ;;
    --dry-run)      DRY_RUN=1; shift ;;
    --check-compat=*) CHECK_COMPAT="${1#*=}"; shift ;;
    --check-compat) CHECK_COMPAT="$2"; shift 2 ;;
    -h|--help)      usage; exit 0 ;;
    -*)             echo "error: unknown flag $1" >&2; exit 4 ;;
    *)
      if [[ -z "$WORKSPACE" ]]; then
        WORKSPACE="$1"
      else
        TOOLCHAINS+=("$1")
      fi
      shift
      ;;
  esac
done

if [[ -z "$WORKSPACE" ]] || [[ ${#TOOLCHAINS[@]} -eq 0 ]]; then
  echo "usage: install-workspace.sh <workspace> [OPTIONS] <toolchain>..." >&2
  exit 4
fi

[[ -d "$WORKSPACE" ]] || { echo "error: not a directory: $WORKSPACE" >&2; exit 2; }
WORKSPACE="$(cd "$WORKSPACE" && pwd)"

case "$UX" in
  ""|crisp|retina|default) ;;
  *) echo "unknown UX preset '$UX'; valid: crisp, retina, default" >&2; exit 4 ;;
esac

case "$CHECK_COMPAT" in
  block|warn|off) ;;
  *) echo "unknown --check-compat='$CHECK_COMPAT' (valid: block, warn, off)" >&2; exit 4 ;;
esac

VSCODE_DIR="$WORKSPACE/.vscode"
EXT_FILE="$VSCODE_DIR/extensions.json"
SETTINGS_FILE="$VSCODE_DIR/settings.json"
TASKS_FILE="$VSCODE_DIR/tasks.json"

if [[ $DRY_RUN -eq 0 ]]; then
  mkdir -p "$VSCODE_DIR"
fi

# --- 0. Pre-install compatibility check (per design.md Decision 11) ---
if [[ "$CHECK_COMPAT" != "off" ]] && [[ $DRY_RUN -eq 0 ]]; then
  compat_script="$ROOT/scripts/check-extension-compatibility.sh"
  if [[ -x "$compat_script" ]] && command -v code >/dev/null 2>&1; then
    incompat_total=0
    incompat_ids=()
    for tc in "${TOOLCHAINS[@]}"; do
      # Use existing flavor-shaped profile dir (post-migration) as the
      # compat target.
      if [[ -d "$ROOT/profiles/$tc" ]]; then
        compat_json=$(bash "$compat_script" --json "$tc" 2>/dev/null || echo '{}')
        n=$(echo "$compat_json" | jq -r '.summary.incompatible // 0')
        if [[ "$n" -gt 0 ]]; then
          ids=$(echo "$compat_json" | jq -r '.extensions[] | select(.status == "incompatible") | .extension')
          incompat_total=$((incompat_total + n))
          while IFS= read -r id; do
            [[ -n "$id" ]] && incompat_ids+=("$id")
          done <<<"$ids"
        fi
      fi
    done
    if [[ $incompat_total -gt 0 ]]; then
      printf 'compat-check: %d incompatible extension(s):\n' "$incompat_total" >&2
      for id in "${incompat_ids[@]}"; do printf '  - %s\n' "$id" >&2; done
      if [[ "$CHECK_COMPAT" == "block" ]]; then
        echo "compat-check: --check-compat=block aborts before any write" >&2
        exit 2
      fi
      echo "compat-check: --check-compat=warn — proceeding with install" >&2
    fi
  fi
fi

# --- 1. Compose extensions and merge into .vscode/extensions.json ---
composed=$(compose_extensions "${TOOLCHAINS[@]}")
ext_ids=$(echo "$composed" | jq '[.[].identifier.id]')

if [[ $DRY_RUN -eq 1 ]]; then
  echo "DRY-RUN: would write $EXT_FILE with $(echo "$ext_ids" | jq length) recommendations:"
  echo "$ext_ids" | jq -r '.[]' | sed 's/^/  - /'
else
  # Merge into existing recommendations (set union, existing first).
  existing_recs="[]"
  if [[ -f "$EXT_FILE" ]]; then
    existing_recs=$(awk -f "$LIB/jsonc-strip.awk" "$EXT_FILE" \
                    | jq '.recommendations // []')
  fi
  merged_recs=$(jq -n --argjson e "$existing_recs" --argjson n "$ext_ids" \
                '$e + ($n - $e)')
  new_doc=$(jq -n --argjson r "$merged_recs" '{recommendations: $r}')
  jsonc_merge "$EXT_FILE" "$new_doc"
  echo "wrote $EXT_FILE ($(echo "$merged_recs" | jq length) recommendations)"
fi

# --- 2. Compose settings: editor-base + optional UX + rust hover + rtk ---
settings='{}'

# editor-base layer (always)
if [[ -f "$ROOT/_shared/editor-base.jsonc" ]]; then
  base_json=$(awk -f "$LIB/jsonc-strip.awk" "$ROOT/_shared/editor-base.jsonc")
  settings=$(jq -n --argjson a "$settings" --argjson b "$base_json" '$a * $b')
fi

# UX preset
if [[ -n "$UX" ]]; then
  ux_file="$ROOT/_shared/ux/$UX.jsonc"
  if [[ -f "$ux_file" ]]; then
    ux_json=$(awk -f "$LIB/jsonc-strip.awk" "$ux_file")
    settings=$(jq -n --argjson a "$settings" --argjson b "$ux_json" '$a * $b')
  fi
fi

# Raw UX overrides
[[ -n "$FONT" ]]       && settings=$(echo "$settings" | jq --arg v "$FONT"       '. + {"editor.fontFamily": $v}')
[[ -n "$FONT_SIZE" ]]  && settings=$(echo "$settings" | jq --argjson v "$FONT_SIZE" '. + {"editor.fontSize": $v}')
[[ -n "$THEME" ]]      && settings=$(echo "$settings" | jq --arg v "$THEME"      '. + {"workbench.colorTheme": $v}')
[[ -n "$ICON_THEME" ]] && settings=$(echo "$settings" | jq --arg v "$ICON_THEME" '. + {"workbench.iconTheme": $v}')

# Rust toolchain settings
RUST_IN_SCOPE=0
for tc in "${TOOLCHAINS[@]}"; do
  [[ "$tc" == "rust" ]] && RUST_IN_SCOPE=1
done
if [[ $RUST_IN_SCOPE -eq 1 ]]; then
  rust_settings=$(jq -r '.settings' "$ROOT/openspec/changes/workspace-toolchain-and-ux-layering/snapshots/rust-tasks.json")
  settings=$(jq -n --argjson a "$settings" --argjson b "$rust_settings" '$a * $b')
fi

# rtk terminal profile (unless --no-rtk)
if [[ $NO_RTK -eq 0 ]]; then
  fish_path=$(command -v fish || true)
  bash_path=$(command -v bash || echo "/bin/bash")

  rtk_settings='{}'

  if [[ -n "$fish_path" ]]; then
    rtk_settings=$(jq -n --arg fp "$fish_path" --arg bp "$bash_path" '
      {
        "terminal.integrated.profiles.osx": {
          "rtk-fish": {
            "path": $fp,
            "args": ["--init-command", "source ${workspaceFolder}/_shared/rtk/rtk-init.fish"],
            "icon": "terminal",
            "color": "terminal.ansiCyan",
            "overrideName": true
          },
          "rtk-bash": {
            "path": $bp,
            "args": ["--rcfile", "${workspaceFolder}/_shared/rtk/rtk-init.bash", "-i"],
            "icon": "terminal",
            "overrideName": true
          }
        },
        "terminal.integrated.profiles.linux": {
          "rtk-fish": {
            "path": $fp,
            "args": ["--init-command", "source ${workspaceFolder}/_shared/rtk/rtk-init.fish"]
          },
          "rtk-bash": {
            "path": $bp,
            "args": ["--rcfile", "${workspaceFolder}/_shared/rtk/rtk-init.bash", "-i"]
          }
        },
        "terminal.integrated.defaultProfile.osx": "rtk-fish",
        "terminal.integrated.defaultProfile.linux": "rtk-fish",
        "terminal.integrated.automationProfile.osx": {
          "path": $fp,
          "args": ["--init-command", "source ${workspaceFolder}/_shared/rtk/rtk-init.fish"]
        },
        "terminal.integrated.env.osx": {"ARTAGON_RTK_REQUIRED": "1"}
      }')
  else
    # Fish absent — fall back to rtk-bash as default.
    rtk_settings=$(jq -n --arg bp "$bash_path" '
      {
        "terminal.integrated.profiles.osx": {
          "rtk-bash": {"path": $bp, "args": ["--rcfile", "${workspaceFolder}/_shared/rtk/rtk-init.bash", "-i"]}
        },
        "terminal.integrated.defaultProfile.osx": "rtk-bash",
        "terminal.integrated.env.osx": {"ARTAGON_RTK_REQUIRED": "1"}
      }')
    echo "note: fish not on PATH; falling back to rtk-bash" >&2
  fi
  settings=$(jq -n --argjson a "$settings" --argjson b "$rtk_settings" '$a * $b')
fi

if [[ $DRY_RUN -eq 1 ]]; then
  echo ""
  echo "DRY-RUN: would write $SETTINGS_FILE ($(echo "$settings" | jq 'keys | length') keys)"
else
  jsonc_merge "$SETTINGS_FILE" "$settings"
  echo "wrote $SETTINGS_FILE ($(echo "$settings" | jq 'keys | length') keys)"
fi

# --- 3. Rust doc tasks ---
if [[ $RUST_IN_SCOPE -eq 1 ]]; then
  rust_tasks=$(jq '{version: "2.0.0", tasks: .tasks}' \
    "$ROOT/openspec/changes/workspace-toolchain-and-ux-layering/snapshots/rust-tasks.json")

  if [[ $DRY_RUN -eq 1 ]]; then
    echo "DRY-RUN: would write $TASKS_FILE (2 rust doc tasks)"
  elif [[ -f "$TASKS_FILE" ]]; then
    # Overwrite-by-label: drop any existing task whose label matches what we're writing,
    # then append our rust doc tasks. Preserve other tasks.
    existing=$(awk -f "$LIB/jsonc-strip.awk" "$TASKS_FILE")
    new_labels=$(jq '[.tasks[].label]' <<<"$rust_tasks")
    merged=$(jq -n --argjson cur "$existing" --argjson new "$rust_tasks" --argjson labels "$new_labels" '
      ($cur.tasks // []) as $old_tasks
      | ($old_tasks | map(select(.label as $l | ($labels | index($l)) | not))) as $kept
      | $cur + {version: ($cur.version // "2.0.0"), tasks: ($kept + $new.tasks)}
    ')
    jsonc_merge "$TASKS_FILE" "$merged"
    echo "wrote $TASKS_FILE (preserved existing tasks + 2 rust doc tasks)"
  else
    jsonc_merge "$TASKS_FILE" "$rust_tasks"
    echo "wrote $TASKS_FILE (2 rust doc tasks)"
  fi
fi
