#!/usr/bin/env bash
# install-extensions.sh — Catalog-driven, pin-verified, allowlist-enforced extension installer
# and safe catalog/packMembers-gated uninstaller for vspcli reconcile.
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/catalog.sh
source "${LIB_DIR}/catalog.sh"
# shellcheck source=scripts/lib/extension-id.sh
source "${LIB_DIR}/extension-id.sh"
# shellcheck source=scripts/lib/vsix-pin.sh
source "${LIB_DIR}/vsix-pin.sh"
# shellcheck source=scripts/lib/legacy-profile-name.sh
source "${LIB_DIR}/legacy-profile-name.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB_DIR}/module-resolver.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB_DIR}/compiler.sh"

usage() {
  cat <<'EOF'
Usage: install-extensions.sh <profile|all> [options]
Options:
  --target=profile|global   Install into named profile (default) or global default profile
  --group <name>            Install only extensions in catalog group (repeatable)
  --without-group <name>    Skip extensions in catalog group (repeatable)
  --uninstall <id>          Uninstall a catalog-known extension or pack member (repeatable)
  --no-batch                Install extensions one-by-one instead of batched
  --dry-run                 Print planned actions without invoking code
  -h, --help                Show help
EOF
}

if [[ $# -lt 1 ]]; then
  usage >&2
  exit 1
fi

PROFILE=""
TARGET="profile"
DRY_RUN=0
EXACT_PROFILE=0
BATCH_INSTALL="${VSCODE_BATCH_INSTALL:-1}"
FILTER_GROUPS=()
EXCLUDE_GROUPS=()
UNINSTALL_IDS=()

while [[ $# -gt 0 ]]; do
  case "${1}" in
    -h|--help)
      usage
      exit 0
      ;;
    --exact-profile)
      EXACT_PROFILE=1
      shift
      ;;
    --)
      shift
      if [[ $# -gt 0 ]]; then
        if [[ -z "${PROFILE}" ]]; then
          PROFILE="${1}"
          shift
        fi
        if [[ $# -gt 0 ]]; then
          printf 'install-extensions: unexpected argument %q\n' "${1}" >&2
          exit 4
        fi
      fi
      break
      ;;
    --target=*)
      TARGET="${1#*=}"
      shift
      ;;
    --target)
      [[ $# -ge 2 ]] || { echo "install-extensions: --target requires a value" >&2; exit 4; }
      TARGET="${2}"
      shift 2
      ;;
    --group)
      [[ $# -ge 2 ]] || { echo "install-extensions: --group requires a value" >&2; exit 4; }
      FILTER_GROUPS+=("${2}")
      shift 2
      ;;
    --group=*)
      FILTER_GROUPS+=("${1#*=}")
      shift
      ;;
    --without-group)
      [[ $# -ge 2 ]] || { echo "install-extensions: --without-group requires a value" >&2; exit 4; }
      EXCLUDE_GROUPS+=("${2}")
      shift 2
      ;;
    --without-group=*)
      EXCLUDE_GROUPS+=("${1#*=}")
      shift
      ;;
    --uninstall)
      [[ $# -ge 2 ]] || { echo "install-extensions: --uninstall requires a value" >&2; exit 4; }
      UNINSTALL_IDS+=("${2}")
      shift 2
      ;;
    --uninstall=*)
      UNINSTALL_IDS+=("${1#*=}")
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --no-batch)
      BATCH_INSTALL=0
      shift
      ;;
    -*)
      printf 'install-extensions: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      if [[ -z "${PROFILE}" ]]; then
        PROFILE="${1}"
      else
        printf 'install-extensions: unexpected argument %q\n' "${1}" >&2
        exit 4
      fi
      shift
      ;;
  esac
done

if [[ -z "${PROFILE}" ]]; then
  usage >&2
  exit 1
fi

if [[ "${TARGET}" != "profile" && "${TARGET}" != "global" ]]; then
  printf 'install-extensions: invalid --target %q (expected profile or global)\n' "${TARGET}" >&2
  exit 4
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "install-extensions: jq is required" >&2
  exit 1
fi
if [[ "${DRY_RUN}" -eq 0 ]] && ! command -v code >/dev/null 2>&1; then
  echo "install-extensions: VS Code 'code' CLI is required on PATH" >&2
  exit 1
fi

# Verify single-store consistency before mutating VS Code.
# shellcheck disable=SC2034 # Out-variable populated by resolve_code_store in scripts/lib/common.sh.
CODE_STORE_USER_DIR=""
CODE_STORE_DATA_ROOT=""
if [[ "${DRY_RUN}" -eq 0 ]]; then
  resolve_code_store --require-data-root || exit $?
else
  resolve_code_store || true
fi

run_code_cmd() {
  local args=()
  if [[ -n "${CODE_STORE_DATA_ROOT}" ]]; then
    args+=(--user-data-dir "${CODE_STORE_DATA_ROOT}")
  fi
  if [[ "${TARGET}" == "profile" && "${PROFILE}" != "default" ]]; then
    args+=(--profile "${PROFILE}")
  fi
  args+=("${@+"${@}"}")
  code "${args[@]}"
}

# Direct --uninstall path runs BEFORE 'all' expansion or legacy profile-name rewriting
# so exact registered profile names (e.g. 'rust-profile-crisp' or 'all') are targeted literally!
if [[ "${#UNINSTALL_IDS[@]}" -gt 0 ]]; then
  if [[ "${PROFILE}" == "all" && "${EXACT_PROFILE}" -eq 0 ]]; then
    echo "install-extensions: --uninstall cannot be combined with 'all' (pass --exact-profile for a literal profile named 'all')" >&2
    exit 4
  fi
  if [[ "${EXACT_PROFILE}" -eq 0 ]]; then
    legacy_res="$(resolve_legacy_profile_name "${PROFILE}")"
    if [[ -n "${legacy_res}" ]]; then
      PROFILE="${legacy_res%% *}"
    fi
  fi
  failed_un=()
  for uid in "${UNINSTALL_IDS[@]}"; do
    validate_extension_id_syntax "${uid}" || { failed_un+=("${uid}"); continue; }
    if ! catalog_is_known_for_uninstall "${uid}" "${ROOT}"; then
      printf 'Error: refusing to uninstall non-catalog extension %q\n' "${uid}" >&2
      failed_un+=("${uid}")
      continue
    fi
    if [[ "${DRY_RUN}" -eq 1 ]]; then
      printf '[DRY-RUN] uninstall (%s:%s) %s\n' "${TARGET}" "${PROFILE}" "${uid}"
      continue
    fi
    if ! run_code_cmd --uninstall-extension "${uid}"; then
      failed_un+=("${uid}")
    fi
  done
  if [[ "${#failed_un[@]}" -gt 0 ]]; then
    printf 'install-extensions: failed to uninstall: %s\n' "${failed_un[*]}" >&2
    exit 1
  fi
  exit 0
fi

# Handle `all`: propagate --target, --dry-run, --group, --without-group to every profile!
if [[ "${PROFILE}" == "all" && "${EXACT_PROFILE}" -eq 0 ]]; then
  failed=0
  all_profiles=""
  if ! all_profiles="$(discover_profiles "${ROOT}")"; then
    echo "install-extensions: failed to discover profiles" >&2
    exit 1
  fi
  while IFS= read -r prof_name; do
    [[ -n "${prof_name}" ]] || continue
    cmd=("${BASH}" "${0}" "${prof_name}" "--target=${TARGET}")
    if [[ "${DRY_RUN}" -eq 1 ]]; then
      cmd+=(--dry-run)
    fi
    if [[ "${BATCH_INSTALL}" == "0" ]]; then
      cmd+=(--no-batch)
    fi
    local_g=""
    for local_g in "${FILTER_GROUPS[@]+"${FILTER_GROUPS[@]}"}"; do
      cmd+=(--group "${local_g}")
    done
    for local_g in "${EXCLUDE_GROUPS[@]+"${EXCLUDE_GROUPS[@]}"}"; do
      cmd+=(--without-group "${local_g}")
    done
    if ! "${cmd[@]}"; then
      failed=1
    fi
  done <<< "${all_profiles}"
  exit "${failed}"
fi

# Resolve legacy profile names cleanly unless --exact-profile was requested.
if [[ "${EXACT_PROFILE}" -eq 0 ]]; then
  legacy_res="$(resolve_legacy_profile_name "${PROFILE}")"
  if [[ -n "${legacy_res}" ]]; then
    PROFILE="${legacy_res%% *}"
  fi
fi

EXT_FILE="${ROOT}/profiles/${PROFILE}/extensions.json"
TMP_EXT_FILE=""
VSIX_TMP=""
trap '[[ -n "${TMP_EXT_FILE}" ]] && rm -f "${TMP_EXT_FILE}"; [[ -n "${VSIX_TMP}" ]] && rm -rf "${VSIX_TMP}"' EXIT

if [[ -f "${ROOT}/profiles/${PROFILE}/profile.json" ]]; then
  # Always compile authoritative profile.json on demand rather than reusing stale ignored extensions.json.
  TMP_EXT_FILE="$(mktemp "${TMPDIR:-/tmp}/vsp-inst-ext.XXXXXX")"
  if ! compiler_compile_profile_extensions_file "${ROOT}" "${PROFILE}" "${TMP_EXT_FILE}"; then
    printf 'install-extensions: failed to compile extensions on-demand for profile %q\n' "${PROFILE}" >&2
    exit 1
  fi
  EXT_FILE="${TMP_EXT_FILE}"
elif [[ ! -f "${EXT_FILE}" ]]; then
  printf 'install-extensions: missing extensions file: %s\n' "${EXT_FILE}" >&2
  exit 1
fi

if ! jq -e -s '
  length == 1
  and (.[0] | type == "array")
  and (all(.[0][]; type == "object" and (.identifier | type == "object") and (.identifier.id | type == "string") and ((.identifier.id | length) > 0)))
' "${EXT_FILE}" >/dev/null; then
  printf 'install-extensions: invalid or malformed extensions JSON array: %s\n' "${EXT_FILE}" >&2
  exit 1
fi

raw_ids=""
if ! raw_ids="$(jq -r -s '.[0][].identifier.id' "${EXT_FILE}")"; then
  printf 'install-extensions: failed to parse extension IDs from: %s\n' "${EXT_FILE}" >&2
  exit 1
fi

ALL_IDS=()
while IFS= read -r id; do
  [[ -n "${id}" ]] && ALL_IDS+=("${id}")
done <<< "${raw_ids}"

if [[ "${#ALL_IDS[@]}" -eq 0 ]]; then
  exit 0
fi

GROUP_ORDER=(AI Themes DX CMake Java Rust Python Web Shell Markdown Quality Git Containers Collaboration General)

is_known_group() {
  local want_lc
  want_lc="$(to_lower "${1}")"
  local g
  for g in "${GROUP_ORDER[@]}"; do
    if [[ "$(to_lower "${g}")" == "${want_lc}" ]]; then
      return 0
    fi
  done
  return 1
}

ALLOWED_GROUPS=()
for g in "${FILTER_GROUPS[@]+"${FILTER_GROUPS[@]}"}"; do
  if is_known_group "${g}"; then
    ALLOWED_GROUPS+=("$(to_lower "${g}")")
  else
    printf 'Warning: unknown group %q — skipping\n' "${g}" >&2
  fi
done

EXCLUDED_GROUPS_LC=()
for g in "${EXCLUDE_GROUPS[@]+"${EXCLUDE_GROUPS[@]}"}"; do
  EXCLUDED_GROUPS_LC+=("$(to_lower "${g}")")
done

group_selected() {
  local g_lc
  g_lc="$(to_lower "${1}")"
  local ex
  for ex in "${EXCLUDED_GROUPS_LC[@]+"${EXCLUDED_GROUPS_LC[@]}"}"; do
    [[ "${ex}" == "${g_lc}" ]] && return 1
  done
  if [[ "${#FILTER_GROUPS[@]}" -eq 0 ]]; then
    return 0
  fi
  local inc
  for inc in "${ALLOWED_GROUPS[@]+"${ALLOWED_GROUPS[@]}"}"; do
    [[ "${inc}" == "${g_lc}" ]] && return 0
  done
  return 1
}

DELAY="${VSCODE_EXTENSION_INSTALL_DELAY:-1}"
PINS_FILE="$(pins_file_path "${ROOT}")"
export VSCODE_VSIX_CACHE_DIR="${VSCODE_VSIX_CACHE_DIR-${ROOT}/.cache/vsix}"
VSIX_TMP="$(mktemp -d "${TMPDIR:-/tmp}/vsp-vsix.XXXXXX")"

FAILED=()
BATCH_IDS=()
BATCH_TARGETS=()
for grp in "${GROUP_ORDER[@]}"; do
  group_selected "${grp}" || continue
  for ext_id in "${ALL_IDS[@]}"; do
    ext_grp="$(catalog_extension_group "${ext_id}" "${ROOT}")"
    if [[ "$(to_lower "${ext_grp}")" != "$(to_lower "${grp}")" ]]; then
      continue
    fi

    if ! validate_extension_id "${ext_id}"; then
      FAILED+=("${ext_id}")
      continue
    fi

    if [[ "${DRY_RUN}" -eq 1 ]]; then
      printf '[DRY-RUN] target=%s profile=%s [%s] %s\n' "${TARGET}" "${PROFILE}" "${grp}" "${ext_id}"
      continue
    fi

    install_target="${ext_id}"
    if ! bypass_pins_active; then
      pin_out=""
      if ! pin_out="$(lookup_pin "${ext_id}" "${PINS_FILE}")"; then
        FAILED+=("${ext_id}")
        continue
      fi
      if [[ -n "${pin_out}" ]]; then
        pin_sha="$(printf '%s\n' "${pin_out}" | sed -n '2p')"
        pin_url="$(printf '%s\n' "${pin_out}" | sed -n '3p')"
        vsix_path="${VSIX_TMP}/${ext_id}.vsix"
        if ! fetch_and_verify_vsix "${ext_id}" "${pin_url}" "${pin_sha}" "${vsix_path}"; then
          FAILED+=("${ext_id}")
          continue
        fi
        install_target="${vsix_path}"
      fi
    fi

    BATCH_IDS+=("${ext_id}")
    BATCH_TARGETS+=("${install_target}")
  done
done

if [[ "${DRY_RUN}" -eq 0 && "${#BATCH_TARGETS[@]}" -gt 0 ]]; then
  if [[ "${BATCH_INSTALL}" == "1" && "${#BATCH_TARGETS[@]}" -gt 1 ]]; then
    batch_flags=()
    for bt in "${BATCH_TARGETS[@]}"; do
      batch_flags+=(--install-extension "${bt}")
    done
    if ! run_code_cmd "${batch_flags[@]}"; then
      idx=0
      while [[ "${idx}" -lt "${#BATCH_TARGETS[@]}" ]]; do
        if ! run_code_cmd --install-extension "${BATCH_TARGETS[${idx}]}"; then
          FAILED+=("${BATCH_IDS[${idx}]}")
        fi
        if [[ "${DELAY}" != "0" ]]; then
          sleep "${DELAY}"
        fi
        idx=$((idx + 1))
      done
    fi
  else
    idx=0
    while [[ "${idx}" -lt "${#BATCH_TARGETS[@]}" ]]; do
      if ! run_code_cmd --install-extension "${BATCH_TARGETS[${idx}]}"; then
        FAILED+=("${BATCH_IDS[${idx}]}")
      fi
      if [[ "${DELAY}" != "0" ]]; then
        sleep "${DELAY}"
      fi
      idx=$((idx + 1))
    done
  fi
fi

if [[ "${#FAILED[@]}" -gt 0 ]]; then
  printf 'install-extensions: failed to install %d extension(s):\n' "${#FAILED[@]}" >&2
  printf '  - %s\n' "${FAILED[@]}" >&2
  exit 1
fi
exit 0
