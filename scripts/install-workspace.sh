#!/usr/bin/env bash
# install-workspace.sh — Unified workspace & profile-target applicator backed by
# scripts/lib/{module-resolver,compiler,jsonc-merge,vscode-storage}.sh.
# Pure Bash 3.2 + jq (zero openspec/changes/ snapshot references).
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/catalog.sh
source "${LIB_DIR}/catalog.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB_DIR}/module-resolver.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB_DIR}/compiler.sh"
# shellcheck source=scripts/lib/jsonc-merge.sh
source "${LIB_DIR}/jsonc-merge.sh"
# shellcheck source=scripts/lib/vscode-storage.sh
source "${LIB_DIR}/vscode-storage.sh"

TARGET="workspace"
TARGET_SET=0
PROFILE_NAME=""
UX_PRESET=""
UX_TARGET=""
RTK_IN_WORKSPACE=0
NO_RTK=0
ALLOW_COMMENT_LOSS=0
CHECK_COMPAT="warn"
CHECK_COMPAT_EXPLICIT=0
DRY_RUN=0
NO_DETECT=0
FORCE=0
FONT_OVERRIDE=""
FONT_SIZE_OVERRIDE=""
THEME_OVERRIDE=""
ICON_THEME_OVERRIDE=""
TOOLCHAINS=()
CLI_WITH=()
CLI_WITHOUT=()
WORKSPACE_PATH=""

usage() {
  cat <<'EOF'
Usage: install-workspace.sh [options] [workspace-path]
Options:
  --target=workspace|profile|global   Apply target (default: workspace)
  --profile <name>                    Named VS Code profile (for --target=profile or UX target)
  --toolchain <id>                    Explicit toolchain/module override (repeatable)
  --no-detect                         Do not auto-detect toolchains (requires --toolchain or --with)
  --with <csv>                        Add capability/toolchain modules (repeatable)
  --without <csv>                     Exclude capability/toolchain modules (repeatable)
  --ux=<crisp|retina|default>         UX preset
  --ux-target=<workspace|user|profile:<name>>  Required when --ux is used with --target=workspace
  --ux-in-workspace                   Shorthand for --ux-target=workspace
  --font <family>                     Override editor.fontFamily (requires UX target in workspace mode)
  --font-size <num>                   Override editor.fontSize
  --theme <label>                     Override workbench.colorTheme
  --icon-theme <id>                   Override workbench.iconTheme
  --rtk-in-workspace                  Emit RTK terminal profiles and rtk task wrappers in workspace
  --no-rtk                            Skip RTK wrapper emission
  --check-compat=off|warn|block       Pre-write extension compatibility gate (default: warn; skipped on --dry-run unless explicit)
  --allow-comment-loss                Allow overwriting interior comments in user settings after backup
  --force                             Allow storage.json mutation while VS Code is running
  --dry-run                           Show planned writes without mutating any files
EOF
}

need_val() {
  if [[ $# -lt 2 || -z "${2}" ]]; then
    printf 'install-workspace: %s requires a value\n' "${1}" >&2
    exit 4
  fi
}

while [[ $# -gt 0 ]]; do
  case "${1}" in
    -h|--help) usage; exit 0 ;;
    --target=*)
      [[ "${TARGET_SET}" -eq 0 ]] || { echo "install-workspace: --target accepts only one value" >&2; exit 4; }
      TARGET="${1#*=}"; TARGET_SET=1; shift ;;
    --target)
      need_val "${1}" "${2:-}"
      [[ "${TARGET_SET}" -eq 0 ]] || { echo "install-workspace: --target accepts only one value" >&2; exit 4; }
      TARGET="${2}"; TARGET_SET=1; shift 2 ;;
    --profile=*) PROFILE_NAME="${1#*=}"; shift ;;
    --profile)   need_val "${1}" "${2:-}"; PROFILE_NAME="${2}"; shift 2 ;;
    --toolchain=*) TOOLCHAINS+=("${1#*=}"); shift ;;
    --toolchain)   need_val "${1}" "${2:-}"; TOOLCHAINS+=("${2}"); shift 2 ;;
    --no-detect) NO_DETECT=1; shift ;;
    --with=*)    CLI_WITH+=("${1#*=}"); shift ;;
    --with)      need_val "${1}" "${2:-}"; CLI_WITH+=("${2}"); shift 2 ;;
    --without=*) CLI_WITHOUT+=("${1#*=}"); shift ;;
    --without)   need_val "${1}" "${2:-}"; CLI_WITHOUT+=("${2}"); shift 2 ;;
    --ux=*)      UX_PRESET="${1#*=}"; shift ;;
    --ux)        need_val "${1}" "${2:-}"; UX_PRESET="${2}"; shift 2 ;;
    --ux-target=*) UX_TARGET="${1#*=}"; shift ;;
    --ux-target)   need_val "${1}" "${2:-}"; UX_TARGET="${2}"; shift 2 ;;
    --ux-in-workspace) UX_TARGET="workspace"; shift ;;
    --font=*)      FONT_OVERRIDE="${1#*=}"; shift ;;
    --font)        need_val "${1}" "${2:-}"; FONT_OVERRIDE="${2}"; shift 2 ;;
    --font-size=*) FONT_SIZE_OVERRIDE="${1#*=}"; shift ;;
    --font-size)   need_val "${1}" "${2:-}"; FONT_SIZE_OVERRIDE="${2}"; shift 2 ;;
    --theme=*)     THEME_OVERRIDE="${1#*=}"; shift ;;
    --theme)       need_val "${1}" "${2:-}"; THEME_OVERRIDE="${2}"; shift 2 ;;
    --icon-theme=*) ICON_THEME_OVERRIDE="${1#*=}"; shift ;;
    --icon-theme)   need_val "${1}" "${2:-}"; ICON_THEME_OVERRIDE="${2}"; shift 2 ;;
    --rtk-in-workspace) RTK_IN_WORKSPACE=1; shift ;;
    --no-rtk)      NO_RTK=1; shift ;;
    --check-compat=*) CHECK_COMPAT="${1#*=}"; CHECK_COMPAT_EXPLICIT=1; shift ;;
    --check-compat)   need_val "${1}" "${2:-}"; CHECK_COMPAT="${2}"; CHECK_COMPAT_EXPLICIT=1; shift 2 ;;
    --allow-comment-loss) ALLOW_COMMENT_LOSS=1; shift ;;
    --force)       FORCE=1; shift ;;
    --dry-run)     DRY_RUN=1; shift ;;
    -*)
      printf 'install-workspace: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      if [[ -z "${WORKSPACE_PATH}" ]]; then
        WORKSPACE_PATH="${1}"
      else
        printf 'install-workspace: unexpected argument %q\n' "${1}" >&2
        exit 4
      fi
      shift
      ;;
  esac
done

case "${TARGET}" in
  workspace|profile|global) ;;
  *) printf 'install-workspace: invalid --target %q (expected workspace, profile, or global)\n' "${TARGET}" >&2; exit 4 ;;
esac

case "${CHECK_COMPAT}" in
  off|warn|block) ;;
  *) printf 'install-workspace: invalid --check-compat %q (expected off, warn, or block)\n' "${CHECK_COMPAT}" >&2; exit 4 ;;
esac

if [[ -n "${UX_PRESET}" ]]; then
  case "${UX_PRESET}" in
    crisp|retina|default) ;;
    *) printf 'install-workspace: invalid --ux %q (valid presets: crisp, retina, default)\n' "${UX_PRESET}" >&2; exit 4 ;;
  esac
fi

HAS_UX_FLAGS=0
if [[ -n "${UX_PRESET}" || -n "${FONT_OVERRIDE}" || -n "${FONT_SIZE_OVERRIDE}" || -n "${THEME_OVERRIDE}" || -n "${ICON_THEME_OVERRIDE}" ]]; then
  HAS_UX_FLAGS=1
fi

# Explicit UX target routing for --target=workspace (never silently mutate global User settings).
if [[ "${TARGET}" == "workspace" && "${HAS_UX_FLAGS}" -eq 1 ]]; then
  if [[ -z "${UX_TARGET}" && -n "${PROFILE_NAME}" ]]; then
    UX_TARGET="profile:${PROFILE_NAME}"
  fi
  if [[ -z "${UX_TARGET}" ]]; then
    echo "install-workspace: --ux/--font/--theme flags with --target=workspace require an explicit destination: pass --ux-in-workspace (--ux-target=workspace), --ux-target=profile:<name> (--profile <name>), or --ux-target=user" >&2
    exit 4
  fi
  case "${UX_TARGET}" in
    workspace|user|profile:*) ;;
    *) printf 'install-workspace: invalid --ux-target %q (expected workspace, user, or profile:<name>)\n' "${UX_TARGET}" >&2; exit 4 ;;
  esac
fi

if [[ "${TARGET}" == "profile" && -z "${PROFILE_NAME}" ]]; then
  if [[ -n "${WORKSPACE_PATH}" && ! -d "${WORKSPACE_PATH}" ]]; then
    PROFILE_NAME="${WORKSPACE_PATH}"
    WORKSPACE_PATH=""
  else
    echo "install-workspace: --target=profile requires --profile <name>" >&2
    exit 4
  fi
fi

# Determine roots (from workspace detection, --toolchain, or profile recipe).
DETECTED_MODS=()
if [[ "${TARGET}" == "workspace" ]]; then
  [[ -n "${WORKSPACE_PATH}" ]] || WORKSPACE_PATH="."
  [[ -d "${WORKSPACE_PATH}" ]] || { printf 'install-workspace: workspace not found: %s\n' "${WORKSPACE_PATH}" >&2; exit 2; }
  WS_CANON="$(canonical_path "${WORKSPACE_PATH}")" || exit 2

  if [[ "${#TOOLCHAINS[@]}" -gt 0 ]]; then
    for tc in "${TOOLCHAINS[@]}"; do
      while IFS= read -r item; do
        [[ -n "${item}" ]] && DETECTED_MODS+=("${item}")
      done < <(printf '%s\n' "${tc}" | tr ',' '\n')
    done
  elif [[ "${NO_DETECT}" -eq 0 ]]; then
    detect_out=""
    detect_rc=0
    detect_out="$("${BASH}" "${SCRIPT_ROOT}/scripts/detect-toolchain.sh" "${WS_CANON}")" || detect_rc=$?
    if [[ "${detect_rc}" -eq 0 ]]; then
      for tok in ${detect_out}; do
        DETECTED_MODS+=("${tok}")
      done
    elif [[ "${detect_rc}" -eq 3 ]]; then
      if [[ "${#CLI_WITH[@]}" -eq 0 ]]; then
        echo "install-workspace: no toolchain detected in ${WORKSPACE_PATH} (pass --toolchain <id> or --with <module>)" >&2
        exit 3
      fi
    else
      exit "${detect_rc}"
    fi
  elif [[ "${#CLI_WITH[@]}" -eq 0 ]]; then
    echo "install-workspace: --no-detect requires --toolchain or --with" >&2
    exit 4
  fi
fi

# Assemble root modules + CLI --with/--without.
ROOTS_STR=""
RECIPE_EXCLUDES=""
RECIPE_OVERRIDES_WS="{}"
RECIPE_OVERRIDES_PROF="{}"

_load_recipe_into_roots() {
  local recipe="${1}"
  local use_def rmods rex rws rpr
  use_def="$(jq -r 'if .useDefaults == false then "false" else "true" end' "${recipe}")"
  if [[ "${use_def}" == "true" ]]; then
    ROOTS_STR="${ROOTS_STR:+${ROOTS_STR}$'\n'}$(catalog_default_capabilities "${ROOT}")"
  fi
  rmods="$(jq -r '.modules[]?' "${recipe}")"
  [[ -n "${rmods}" ]] && ROOTS_STR="${ROOTS_STR:+${ROOTS_STR}$'\n'}${rmods}"
  rex="$(jq -r '.excludeModules[]?' "${recipe}")"
  [[ -n "${rex}" ]] && RECIPE_EXCLUDES="${RECIPE_EXCLUDES:+${RECIPE_EXCLUDES}$'\n'}${rex}"
  rws="$(jq -c 'if (.overrides | type) == "object" and (.overrides | has("workspace") or has("profile")) then (.overrides.workspace // {}) else {} end' "${recipe}")"
  rpr="$(jq -c 'if (.overrides | type) == "object" and (.overrides | has("workspace") or has("profile")) then (.overrides.profile // {}) else (.overrides // {}) end' "${recipe}")"
  RECIPE_OVERRIDES_WS="$(jq -n --argjson a "${RECIPE_OVERRIDES_WS}" --argjson b "${rws}" '$a * $b')"
  RECIPE_OVERRIDES_PROF="$(jq -n --argjson a "${RECIPE_OVERRIDES_PROF}" --argjson b "${rpr}" '$a * $b')"
}

if [[ "${TARGET}" != "workspace" && "${#TOOLCHAINS[@]}" -gt 0 ]]; then
  for tc in "${TOOLCHAINS[@]}"; do
    while IFS= read -r item; do
      [[ -n "${item}" ]] || continue
      if [[ -f "${ROOT}/profiles/${item}/profile.json" ]]; then
        _load_recipe_into_roots "${ROOT}/profiles/${item}/profile.json"
      else
        if [[ -z "${ROOTS_STR}" ]]; then
          ROOTS_STR="$(catalog_default_capabilities "${ROOT}")"
        fi
        ROOTS_STR="${ROOTS_STR:+${ROOTS_STR}$'\n'}${item}"
      fi
    done < <(printf '%s\n' "${tc}" | tr ',' '\n')
  done
elif [[ "${TARGET}" == "profile" && -f "${ROOT}/profiles/${PROFILE_NAME}/profile.json" ]]; then
  _load_recipe_into_roots "${ROOT}/profiles/${PROFILE_NAME}/profile.json"
else
  ROOTS_STR="$(catalog_default_capabilities "${ROOT}")"
fi

for m in "${DETECTED_MODS[@]+"${DETECTED_MODS[@]}"}"; do
  ROOTS_STR="${ROOTS_STR:+${ROOTS_STR}$'\n'}${m}"
done

CLI_WITH_STR="$(printf '%s\n' "${CLI_WITH[@]+"${CLI_WITH[@]}"}")"
CLI_WITHOUT_STR="$(printf '%s\n' "${CLI_WITHOUT[@]+"${CLI_WITHOUT[@]}"}")"

RESOLVED_STR=""
RESOLVED_STR="$(resolve_modules "${ROOT}" \
  --roots "${ROOTS_STR}" \
  --exclude "${RECIPE_EXCLUDES}" \
  --cli-with "${CLI_WITH_STR}" \
  --cli-without "${CLI_WITHOUT_STR}")" || exit $?

RESOLVED_MODS=()
while IFS= read -r m; do
  [[ -n "${m}" ]] && RESOLVED_MODS+=("${m}")
done <<< "${RESOLVED_STR}"

# Build UX overlay JSON (and deletion key list when --ux=default).
build_ux_overlay() {
  local ux_json="{}"
  if [[ "${UX_PRESET}" == "crisp" || "${UX_PRESET}" == "retina" ]]; then
    ux_json="$(compiler_parse_jsonc_object "${ROOT}" "${ROOT}/modules/ux/${UX_PRESET}/settings.profile.jsonc")" || return 1
  fi
  if [[ -n "${FONT_OVERRIDE}" ]]; then
    ux_json="$(printf '%s' "${ux_json}" | jq --arg v "${FONT_OVERRIDE}" '."editor.fontFamily" = $v')"
  fi
  if [[ -n "${FONT_SIZE_OVERRIDE}" ]]; then
    if [[ ! "${FONT_SIZE_OVERRIDE}" =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
      printf 'install-workspace: --font-size must be numeric (got %q)\n' "${FONT_SIZE_OVERRIDE}" >&2
      return 4
    fi
    ux_json="$(printf '%s' "${ux_json}" | jq --argjson v "${FONT_SIZE_OVERRIDE}" '."editor.fontSize" = $v')"
  fi
  if [[ -n "${THEME_OVERRIDE}" ]]; then
    ux_json="$(printf '%s' "${ux_json}" | jq --arg v "${THEME_OVERRIDE}" '."workbench.colorTheme" = $v | ."workbench.preferredDarkColorTheme" = $v')"
  fi
  if [[ -n "${ICON_THEME_OVERRIDE}" ]]; then
    ux_json="$(printf '%s' "${ux_json}" | jq --arg v "${ICON_THEME_OVERRIDE}" '."workbench.iconTheme" = $v')"
  fi
  printf '%s\n' "${ux_json}"
}

UX_OVERLAY_JSON="$(build_ux_overlay)" || exit $?
UX_DELETE_KEYS_JSON="[]"
if [[ "${UX_PRESET}" == "default" ]]; then
  UX_DELETE_KEYS_JSON="$(compiler_ux_managed_keys_json "${ROOT}")" || exit 1
fi

# Compile settings, extensions, and tasks in memory BEFORE any filesystem writes!
WORK_TMP="$(mktemp -d "${TMPDIR:-/tmp}/vsp-apply.XXXXXX")"
trap 'rm -rf "${WORK_TMP}"' EXIT

if [[ "${TARGET}" == "workspace" ]]; then
  WS_EXT_OBJ="$(compiler_compile_extensions "${ROOT}" workspace "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}")" || exit 1
  WS_EXT_IDS_FILE="${WORK_TMP}/resolved-ext-ids.txt"
  printf '%s\n' "${WS_EXT_OBJ}" | jq -r '.recommendations[]?' > "${WS_EXT_IDS_FILE}"
else
  PROF_EXT_ARR="$(compiler_compile_extensions "${ROOT}" profile "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}")" || exit 1
  WS_EXT_IDS_FILE="${WORK_TMP}/resolved-ext-ids.txt"
  printf '%s\n' "${PROF_EXT_ARR}" | jq -r '.[].identifier.id // empty' > "${WS_EXT_IDS_FILE}"
fi

# Pre-write compatibility gate (runs BEFORE creating .vscode/ or writing XDG/VS Code files!).
run_compat_gate() {
  [[ "${CHECK_COMPAT}" == "off" ]] && return 0
  [[ -s "${WS_EXT_IDS_FILE}" ]] || return 0

  local checker="${VSPCLI_COMPAT_CHECKER:-${SCRIPT_ROOT}/scripts/check-extension-compatibility.sh}"
  if [[ ! -f "${checker}" ]]; then
    echo "compat-check: checker script missing: ${checker}" >&2
    [[ "${CHECK_COMPAT}" == "block" ]] && return 1
    return 0
  fi

  local compat_out=""
  local compat_rc=0
  set +e
  compat_out="$("${BASH}" "${checker}" --extensions-file "${WS_EXT_IDS_FILE}" --marketplace-only --json 2>"${WORK_TMP}/compat.err")"
  compat_rc=$?
  set -e

  if [[ -s "${WORK_TMP}/compat.err" ]]; then
    cat "${WORK_TMP}/compat.err" >&2
  fi

  # Parse JSON strictly; fail closed in block mode on malformed JSON, exit 2,
  # missing/mismatched/duplicate IDs, unknown/NOT_FOUND, or incompatible entries.
  local parsed_ok=0
  if printf '%s\n' "${compat_out}" | jq -e -s 'length == 1 and (.[0] | type == "object") and (.[0].summary | type == "object") and (.[0].extensions | type == "array")' >/dev/null 2>&1; then
    parsed_ok=1
  fi

  if [[ "${parsed_ok}" -eq 0 || "${compat_rc}" -eq 2 ]]; then
    printf 'compat-check: unable to verify extension compatibility (exit=%d)\n' "${compat_rc}" >&2
    [[ "${CHECK_COMPAT}" == "block" ]] && return 1
    return 0
  fi

  # Verify exact ID coverage (no missing IDs, mismatched IDs, or duplicate records).
  local req_json
  req_json="$(jq -R . "${WS_EXT_IDS_FILE}" | jq -s 'map(ascii_downcase) | sort | unique')"
  local got_check
  got_check="$(printf '%s\n' "${compat_out}" | jq -r --argjson req "${req_json}" '
    (.extensions | map(.id | ascii_downcase)) as $got
    | ($got | unique | sort) as $got_uniq
    | if ($got | length) != ($got_uniq | length) then "DUPLICATE"
      elif $got_uniq != $req then "MISMATCH"
      else "OK" end
  ')"

  if [[ "${got_check}" != "OK" ]]; then
    printf 'compat-check: incomplete or mismatched compatibility result set (%s)\n' "${got_check}" >&2
    [[ "${CHECK_COMPAT}" == "block" ]] && return 1
    return 0
  fi

  local incompat not_found
  incompat="$(printf '%s\n' "${compat_out}" | jq -r '.summary.incompatible // 0')"
  not_found="$(printf '%s\n' "${compat_out}" | jq -r '.summary.not_found // 0')"

  if [[ "${incompat}" -gt 0 || "${not_found}" -gt 0 || "${compat_rc}" -ne 0 ]]; then
    printf 'compat-check: %d incompatible, %d not_found extension(s)\n' "${incompat}" "${not_found}" >&2
    printf '%s\n' "${compat_out}" | jq -r '.extensions[] | select(.status != "compatible") | "  \(.status): \(.id) (engine=\(.engine // "none"))"' >&2
    if [[ "${CHECK_COMPAT}" == "block" ]]; then
      echo "compat-check: aborting due to --check-compat=block (zero files written)" >&2
      return 1
    fi
  fi
  return 0
}

COMPAT_CHECK_SKIPPED=0
if [[ "${DRY_RUN}" -eq 1 && "${CHECK_COMPAT_EXPLICIT}" -eq 0 ]]; then
  COMPAT_CHECK_SKIPPED=1
else
  run_compat_gate || exit 1
fi

# Handle --target=workspace
if [[ "${TARGET}" == "workspace" ]]; then
  # Containment checks BEFORE any mutation!
  check_path_contained "${WS_CANON}" "${WS_CANON}/.vscode" || exit 4
  check_path_contained "${WS_CANON}" "${WS_CANON}/.vscode/settings.json" || exit 4
  check_path_contained "${WS_CANON}" "${WS_CANON}/.vscode/extensions.json" || exit 4
  check_path_contained "${WS_CANON}" "${WS_CANON}/.vscode/tasks.json" || exit 4

  WS_SETTINGS_JSON="$(compiler_compile_settings "${ROOT}" workspace --override-json "${RECIPE_OVERRIDES_WS}" -- "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}")" || exit 1

  # Apply UX directly into workspace ONLY when --ux-in-workspace / --ux-target=workspace.
  WS_DELETE_KEYS='[]'
  if [[ "${UX_TARGET}" == "workspace" ]]; then
    if [[ "${UX_PRESET}" == "default" ]]; then
      WS_DELETE_KEYS="${UX_DELETE_KEYS_JSON}"
    fi
    WS_SETTINGS_JSON="$(printf '%s\n' "${WS_SETTINGS_JSON}" | jq --argjson ux "${UX_OVERLAY_JSON}" '. * $ux')"
  fi

  # Emit RTK into workspace ONLY when --rtk-in-workspace (and not --no-rtk).
  RTK_TASK_MODE="plain"
  if [[ "${RTK_IN_WORKSPACE}" -eq 1 && "${NO_RTK}" -eq 0 ]]; then
    RTK_TASK_MODE="rtk"
  fi
  WS_TASKS_JSON="$(compiler_compile_tasks "${ROOT}" "${RTK_TASK_MODE}" "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}")" || exit 1

  # Compute XDG_STATE_HOME fingerprinted provenance for idempotent reapplication & formatter/AI key switches.
  STATE_FILE="$(compiler_workspace_state_file "${WS_CANON}")"
  WS_FP="$(compiler_workspace_fingerprint "${WS_CANON}")"
  PREV_MANAGED_RECS='[]'
  PREV_MANAGED_KEYS='[]'
  if [[ -f "${STATE_FILE}" ]]; then
    prev_fp="$(jq -r '.workspaceFingerprint // empty' "${STATE_FILE}" 2>/dev/null || true)"
    if [[ "${prev_fp}" == "${WS_FP}" ]]; then
      PREV_MANAGED_RECS="$(jq -c '.managedRecommendations // []' "${STATE_FILE}" 2>/dev/null || printf '[]')"
      PREV_MANAGED_KEYS="$(jq -c '.managedSettingsKeys // []' "${STATE_FILE}" 2>/dev/null || printf '[]')"
    fi
  fi

  if [[ "${DRY_RUN}" -eq 1 ]]; then
    echo "DRY-RUN: target=workspace path=${WS_CANON}"
    echo "DRY-RUN: modules=${RESOLVED_MODS[*]+"${RESOLVED_MODS[*]}"}"
    if [[ "${COMPAT_CHECK_SKIPPED}" -eq 1 ]]; then
      echo "DRY-RUN: compatibility=unchecked (skipped implicit Marketplace check; pass --check-compat=warn|block to run)"
    elif [[ "${CHECK_COMPAT}" == "off" ]]; then
      echo "DRY-RUN: compatibility=off"
    fi
    echo "DRY-RUN: would write ${WS_CANON}/.vscode/settings.json"
    echo "DRY-RUN: would write ${WS_CANON}/.vscode/extensions.json"
    [[ -n "${WS_TASKS_JSON}" ]] && echo "DRY-RUN: would write ${WS_CANON}/.vscode/tasks.json (mode=${RTK_TASK_MODE})"
    [[ -n "${UX_TARGET}" && "${UX_TARGET}" != "workspace" ]] && echo "DRY-RUN: would apply UX (${UX_PRESET}) to ${UX_TARGET}"
    exit 0
  fi

  if [[ "${RTK_IN_WORKSPACE}" -eq 1 && "${NO_RTK}" -eq 0 ]]; then
    XDG_RTK_DIR="$(compiler_install_rtk_xdg "${ROOT}")" || exit 1
    RTK_PROF_JSON="$(compiler_rtk_terminal_settings_json "${XDG_RTK_DIR}")" || exit 1
    # Guard against leaking literal $HOME absolute host paths into team-shareable .vscode/settings.json.
    ws_home="${HOME%/}"
    ws_home_canon=""
    if [[ -n "${ws_home}" && -d "${ws_home}" ]]; then
      ws_home_canon="$(canonical_path "${ws_home}" 2>/dev/null || true)"
      ws_home_canon="${ws_home_canon%/}"
    fi
    if [[ -n "${ws_home}" && "${ws_home}" != "/" ]]; then
      if printf '%s\n' "${RTK_PROF_JSON}" | jq -e --arg h "${ws_home}" --arg hc "${ws_home_canon}" '
        def matches_home:
          (. == $h or startswith($h + "/"))
          or ($hc != "" and $hc != "/" and (. == $hc or startswith($hc + "/")));
        [.. | strings | select(matches_home)] | length > 0
      ' >/dev/null 2>&1; then
        printf "install-workspace: warning: --rtk-in-workspace rewrote literal \$HOME paths to \${userHome} in .vscode/settings.json (prefer --target=profile or --target=global for personal RTK profiles)\n" >&2
        # shellcheck disable=SC2016
        RTK_PROF_JSON="$(printf '%s\n' "${RTK_PROF_JSON}" | jq --arg h "${ws_home}" --arg hc "${ws_home_canon}" '
          def portabilize:
            if type == "string" then
              if . == $h then "${userHome}"
              elif startswith($h + "/") then "${userHome}" + .[$h | length:]
              elif ($hc != "" and $hc != "/" and . == $hc) then "${userHome}"
              elif ($hc != "" and $hc != "/" and startswith($hc + "/")) then "${userHome}" + .[$hc | length:]
              else . end
            elif type == "array" then map(portabilize)
            elif type == "object" then with_entries(.value |= portabilize)
            else . end;
          portabilize
        ')"
      fi
    fi
    if [[ "${XDG_RTK_DIR}" == /* ]]; then
      rtk_under_home=0
      if [[ -n "${ws_home}" && "${ws_home}" != "/" ]]; then
        if [[ "${XDG_RTK_DIR}" == "${ws_home}" || "${XDG_RTK_DIR}" == "${ws_home}/"* ]]; then
          rtk_under_home=1
        fi
      fi
      if [[ "${rtk_under_home}" -eq 0 && -n "${ws_home_canon}" && "${ws_home_canon}" != "/" ]]; then
        if [[ "${XDG_RTK_DIR}" == "${ws_home_canon}" || "${XDG_RTK_DIR}" == "${ws_home_canon}/"* ]]; then
          rtk_under_home=1
        fi
      fi
      if [[ "${rtk_under_home}" -eq 0 ]]; then
        printf 'install-workspace: warning: --rtk-in-workspace embeds absolute host path (%s) in .vscode/settings.json; avoid committing host-specific paths to shared repos\n' "${XDG_RTK_DIR}" >&2
      fi
    fi
    WS_SETTINGS_JSON="$(printf '%s\n' "${WS_SETTINGS_JSON}" | jq --argjson rtk "${RTK_PROF_JSON}" '. * $rtk')"
  fi

  # Compute managed/retired settings keys AFTER RTK merge so re-runs with --rtk-in-workspace
  # never retire terminal.integrated.profiles.* and clobber custom user profiles.
  NEW_SETTINGS_KEYS="$(printf '%s\n' "${WS_SETTINGS_JSON}" | jq -c 'keys')"
  RETIRE_SETTINGS_KEYS="$(jq -n --argjson prev "${PREV_MANAGED_KEYS}" --argjson cur "${NEW_SETTINGS_KEYS}" --argjson extra "${WS_DELETE_KEYS}" '
    (($prev - $cur) + $extra) | unique
  ')"

  # Compute nested path deletions when switching mutually exclusive JS/TS formatters
  # so stale editor.codeActionsOnSave entries inside [lang] blocks are cleaned cleanly.
  has_biome=0
  has_eslint_prettier=0
  for m in "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}"; do
    [[ "${m}" == "biome" ]] && has_biome=1
    [[ "${m}" == "ts-eslint-prettier" ]] && has_eslint_prettier=1
  done
  WS_DELETE_PATHS='[]'
  if [[ "${has_biome}" -eq 1 && "${has_eslint_prettier}" -eq 0 ]]; then
    WS_DELETE_PATHS='[["[javascript]","editor.codeActionsOnSave","source.fixAll.eslint"],["[typescript]","editor.codeActionsOnSave","source.fixAll.eslint"],["[javascriptreact]","editor.codeActionsOnSave","source.fixAll.eslint"],["[typescriptreact]","editor.codeActionsOnSave","source.fixAll.eslint"]]'
  elif [[ "${has_eslint_prettier}" -eq 1 && "${has_biome}" -eq 0 ]]; then
    WS_DELETE_PATHS='[["[javascript]","editor.codeActionsOnSave","source.fixAll.biome"],["[javascript]","editor.codeActionsOnSave","source.organizeImports.biome"],["[typescript]","editor.codeActionsOnSave","source.fixAll.biome"],["[typescript]","editor.codeActionsOnSave","source.organizeImports.biome"],["[javascriptreact]","editor.codeActionsOnSave","source.fixAll.biome"],["[javascriptreact]","editor.codeActionsOnSave","source.organizeImports.biome"],["[typescriptreact]","editor.codeActionsOnSave","source.fixAll.biome"],["[typescriptreact]","editor.codeActionsOnSave","source.organizeImports.biome"]]'
  fi

  mkdir -p "${WS_CANON}/.vscode"

  # Write .vscode/settings.json preserving unrelated user keys and retiring switched-away managed keys.
  jsonc_merge \
    --workspace-root "${WS_CANON}" \
    --delete-keys-json "${RETIRE_SETTINGS_KEYS}" \
    --delete-paths-json "${WS_DELETE_PATHS}" \
    "${WS_CANON}/.vscode/settings.json" "${WS_SETTINGS_JSON}" || exit $?

  # Write .vscode/extensions.json with fingerprinted provenance:
  #   - preserves pre-existing user-owned recommendations and unrelated JSONC fields
  #   - subtracts ONLY previously vspcli-inserted recommendations no longer in current workspace set
  #   - NEVER subtracts from unwantedRecommendations
  #   - deduplicates case-insensitively and removes any unwanted IDs from recommendations
  EXISTING_EXT_JSON='{}'
  if [[ -f "${WS_CANON}/.vscode/extensions.json" ]]; then
    EXISTING_EXT_JSON="$(LC_ALL=C awk -f "${LIB_DIR}/jsonc-strip.awk" "${WS_CANON}/.vscode/extensions.json")" || exit 1
    printf '%s\n' "${EXISTING_EXT_JSON}" | jq -e -s 'length == 1 and (.[0] | type == "object")' >/dev/null 2>&1 || {
      echo "install-workspace: existing .vscode/extensions.json is not a single valid JSON object" >&2
      exit 1
    }
  fi

  MERGED_EXT_JSON="$(jq -n \
    --argjson existing "${EXISTING_EXT_JSON}" \
    --argjson incoming "${WS_EXT_OBJ}" \
    --argjson prev_managed "${PREV_MANAGED_RECS}" '
    def dedup_ci:
      reduce .[] as $id ({seen: [], out: []};
        ($id | ascii_downcase) as $lc
        | if (.seen | index($lc)) != null then .
          else {seen: (.seen + [$lc]), out: (.out + [$id])}
          end
      ) | .out;
    ($incoming.recommendations // []) as $inc_recs
    | ($inc_recs | map(ascii_downcase)) as $inc_recs_lc
    | ($prev_managed | map(ascii_downcase)) as $prev_lc
    | (($existing.unwantedRecommendations // []) + ($incoming.unwantedRecommendations // []) | dedup_ci) as $final_unwanted
    | ($final_unwanted | map(ascii_downcase)) as $unwanted_lc
    | (($existing.recommendations // [])
        | map(select(
            (ascii_downcase) as $lc
            | (($prev_lc | index($lc)) == null) or (($inc_recs_lc | index($lc)) != null)
          ))
      ) as $kept_existing
    | (($kept_existing + $inc_recs) | dedup_ci
        | map(. as $r | select(($unwanted_lc | index($r | ascii_downcase)) == null))
      ) as $final_recs
    | ($existing
        | .recommendations = $final_recs
        | (if ($final_unwanted | length) > 0 or ($existing | has("unwantedRecommendations")) then
             .unwantedRecommendations = $final_unwanted
           else . end)
      )
  ')"

  printf '%s\n' "${MERGED_EXT_JSON}" | jq . > "${WORK_TMP}/extensions.json"
  atomic_write_file --preserve-mode "${WS_CANON}/.vscode/extensions.json" "${WORK_TMP}/extensions.json" 0644 || exit 1

  # Track which recommendation IDs are vspcli-managed (excluding IDs that already existed before vspcli ran).
  NEW_MANAGED_RECS="$(jq -n \
    --argjson existing "${EXISTING_EXT_JSON}" \
    --argjson incoming "${WS_EXT_OBJ}" \
    --argjson prev_managed "${PREV_MANAGED_RECS}" '
    (($existing.recommendations // []) | map(ascii_downcase)) as $ex_lc
    | ($prev_managed | map(ascii_downcase)) as $prev_lc
    | ($incoming.recommendations // [])
    | map(select(
        (ascii_downcase) as $lc
        | (($ex_lc | index($lc)) == null) or (($prev_lc | index($lc)) != null)
      ))
  ')"

  if [[ -n "${WS_TASKS_JSON}" ]]; then
    if [[ -f "${WS_CANON}/.vscode/tasks.json" ]]; then
      EXISTING_TASKS="$(LC_ALL=C awk -f "${LIB_DIR}/jsonc-strip.awk" "${WS_CANON}/.vscode/tasks.json")" || exit 1
      jq -n --argjson ex "${EXISTING_TASKS}" --argjson inc "${WS_TASKS_JSON}" '
        $ex | .version = ($ex.version // "2.0.0") | .tasks = (
          reduce ($inc.tasks // [])[] as $t (($ex.tasks // []);
            (map(.label) | index($t.label)) as $idx
            | if $idx != null then .[$idx] = $t else . + [$t] end
          )
        )
      ' > "${WORK_TMP}/tasks.json"
    else
      printf '%s\n' "${WS_TASKS_JSON}" | jq . > "${WORK_TMP}/tasks.json"
    fi
    atomic_write_file --preserve-mode "${WS_CANON}/.vscode/tasks.json" "${WORK_TMP}/tasks.json" 0644 || exit 1
  fi

  # Update XDG_STATE_HOME workspace provenance atomically AFTER workspace files succeed.
  FINAL_SETTINGS_KEYS="$(printf '%s\n' "${WS_SETTINGS_JSON}" | jq -c 'keys')"
  jq -n \
    --arg fp "${WS_FP}" \
    --arg ws "${WS_CANON}" \
    --argjson recs "${NEW_MANAGED_RECS}" \
    --argjson keys "${FINAL_SETTINGS_KEYS}" '
    {
      version: 1,
      workspace: $ws,
      workspaceFingerprint: $fp,
      managedRecommendations: $recs,
      managedSettingsKeys: $keys
    }
  ' > "${WORK_TMP}/state.json"
  atomic_write_file --no-follow "${STATE_FILE}" "${WORK_TMP}/state.json" 0600 || exit 1

  # Apply UX to User or Named Profile when explicitly requested via --ux-target=user|profile:<name>.
  if [[ "${UX_TARGET}" == "user" || "${UX_TARGET}" == profile:* ]]; then
    CODE_STORE_USER_DIR=""
    CODE_STORE_DATA_ROOT=""
    resolve_code_store || exit $?
    : "${CODE_STORE_DATA_ROOT}"
    ux_dest=""
    if [[ "${UX_TARGET}" == "user" ]]; then
      ux_dest="${CODE_STORE_USER_DIR}/settings.json"
    else
      ux_prof="${UX_TARGET#profile:}"
      ensure_args=()
      [[ "${FORCE}" -eq 1 ]] && ensure_args+=(--force)
      prof_id="$(vscode_storage_ensure_profile "${ensure_args[@]+"${ensure_args[@]}"}" "${CODE_STORE_USER_DIR}" "${ux_prof}")" || exit 1
      ux_dest="${CODE_STORE_USER_DIR}/profiles/${prof_id}/settings.json"
    fi
    jmerge_args=(--user-root "${CODE_STORE_USER_DIR}" --delete-keys-json "${UX_DELETE_KEYS_JSON}")
    [[ "${ALLOW_COMMENT_LOSS}" -eq 1 ]] && jmerge_args+=(--allow-comment-loss)
    jsonc_merge "${jmerge_args[@]}" "${ux_dest}" "${UX_OVERLAY_JSON}" || exit $?
  fi

  printf 'Applied workspace stack (%s) to %s\n' "${RESOLVED_MODS[*]+"${RESOLVED_MODS[*]}"}" "${WS_CANON}"
  exit 0
fi

# Handle --target=profile or --target=global
COMBINED_OV="$(jq -n --argjson a "${RECIPE_OVERRIDES_WS}" --argjson b "${RECIPE_OVERRIDES_PROF}" '$a * $b')"
PROF_SETTINGS_JSON="$(compiler_compile_settings "${ROOT}" both --override-json "${COMBINED_OV}" -- "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}")" || exit 1

# Clear chat.disableAIFeatures when applying an AI-enabled stack over a prior general-minimal profile.
PROF_DELETE_KEYS="${UX_DELETE_KEYS_JSON}"
has_ai_mod=0
for m in "${RESOLVED_MODS[@]+"${RESOLVED_MODS[@]}"}"; do
  if [[ "${m}" == "ai-agents" || "${m}" == "ai-copilot" ]]; then
    has_ai_mod=1
    break
  fi
done
if [[ "${has_ai_mod}" -eq 1 ]]; then
  PROF_DELETE_KEYS="$(printf '%s' "${PROF_DELETE_KEYS}" | jq '. + ["chat.disableAIFeatures"] | unique')"
fi

if [[ "${UX_PRESET}" == "crisp" || "${UX_PRESET}" == "retina" || "${HAS_UX_FLAGS}" -eq 1 ]]; then
  PROF_SETTINGS_JSON="$(printf '%s\n' "${PROF_SETTINGS_JSON}" | jq --argjson ux "${UX_OVERLAY_JSON}" '. * $ux')"
fi

if [[ "${DRY_RUN}" -eq 1 ]]; then
  echo "DRY-RUN: target=${TARGET} profile=${PROFILE_NAME:-default}"
  echo "DRY-RUN: modules=${RESOLVED_MODS[*]+"${RESOLVED_MODS[*]}"}"
  if [[ "${COMPAT_CHECK_SKIPPED}" -eq 1 ]]; then
    echo "DRY-RUN: compatibility=unchecked (skipped implicit Marketplace check; pass --check-compat=warn|block to run)"
  elif [[ "${CHECK_COMPAT}" == "off" ]]; then
    echo "DRY-RUN: compatibility=off"
  fi
  echo "DRY-RUN: extensions=$(jq -r 'length' <<<"${PROF_EXT_ARR}")"
  exit 0
fi

if [[ "${NO_RTK}" -eq 0 ]]; then
  XDG_RTK_DIR="$(compiler_install_rtk_xdg "${ROOT}")" || exit 1
  RTK_PROF_JSON="$(compiler_rtk_terminal_settings_json "${XDG_RTK_DIR}")" || exit 1
  PROF_SETTINGS_JSON="$(printf '%s\n' "${PROF_SETTINGS_JSON}" | jq --argjson rtk "${RTK_PROF_JSON}" '. * $rtk')"
fi

CODE_STORE_USER_DIR=""
CODE_STORE_DATA_ROOT=""
resolve_code_store || exit $?
: "${CODE_STORE_DATA_ROOT}"

if [[ "${TARGET}" == "global" ]]; then
  DEST_SETTINGS="${CODE_STORE_USER_DIR}/settings.json"
else
  ensure_args=()
  [[ "${FORCE}" -eq 1 ]] && ensure_args+=(--force)
  PROF_ID="$(vscode_storage_ensure_profile "${ensure_args[@]+"${ensure_args[@]}"}" "${CODE_STORE_USER_DIR}" "${PROFILE_NAME}")" || exit 1
  DEST_SETTINGS="${CODE_STORE_USER_DIR}/profiles/${PROF_ID}/settings.json"
fi

jmerge_args=(--user-root "${CODE_STORE_USER_DIR}" --delete-keys-json "${PROF_DELETE_KEYS}")
[[ "${ALLOW_COMMENT_LOSS}" -eq 1 ]] && jmerge_args+=(--allow-comment-loss)
jsonc_merge "${jmerge_args[@]}" "${DEST_SETTINGS}" "${PROF_SETTINGS_JSON}" || exit $?

printf 'Applied %s stack (%s) to %s\n' "${TARGET}" "${RESOLVED_MODS[*]+"${RESOLVED_MODS[*]}"}" "${DEST_SETTINGS}"
exit 0
