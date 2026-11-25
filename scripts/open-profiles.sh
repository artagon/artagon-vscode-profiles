#!/usr/bin/env bash
# open-profiles.sh — Open VS Code profiles, install extensions with pin+store-keyed cache,
# sync _merged/<name>.json preserving local 14-key UX settings, and guard `-- "$path"`.
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB_DIR}/module-resolver.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB_DIR}/compiler.sh"
# shellcheck source=scripts/lib/vscode-storage.sh
source "${LIB_DIR}/vscode-storage.sh"
# shellcheck source=scripts/lib/vsix-pin.sh
source "${LIB_DIR}/vsix-pin.sh"
# shellcheck source=scripts/lib/legacy-profile-name.sh
source "${LIB_DIR}/legacy-profile-name.sh"

SKIP_INSTALL="${VSCODE_SKIP_EXTENSION_INSTALL:-0}"
EXCLUDE_GROUPS=()
REQUESTED=()

while [[ $# -gt 0 ]]; do
  case "${1}" in
    --skip-install)
      SKIP_INSTALL=1
      shift
      ;;
    --without-group)
      [[ $# -ge 2 ]] || { echo "open-profiles: --without-group requires a value" >&2; exit 4; }
      EXCLUDE_GROUPS+=("${2}")
      shift 2
      ;;
    --without-group=*)
      EXCLUDE_GROUPS+=("${1#*=}")
      shift
      ;;
    -*)
      printf 'open-profiles: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      REQUESTED+=("${1}")
      shift
      ;;
  esac
done

if ! command -v code >/dev/null 2>&1; then
  echo "open-profiles: VS Code 'code' CLI is required on PATH" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "open-profiles: jq is required" >&2
  exit 1
fi

CODE_STORE_USER_DIR=""
CODE_STORE_DATA_ROOT=""
resolve_code_store --require-data-root || exit $?
CODE_USER_DIR="${CODE_STORE_USER_DIR}"

CACHE_DIR="${VSCODE_EXT_CACHE_DIR:-${ROOT}/.cache/extensions-installed}"
mkdir -p "${CACHE_DIR}"

PINS_FILE="$(pins_file_path "${ROOT}")"
UX_KEYS_JSON="$(compiler_ux_managed_keys_json "${ROOT}")" || exit 1

PROFILES=()
if [[ "${#REQUESTED[@]}" -gt 0 ]]; then
  for req in "${REQUESTED[@]}"; do
    legacy="$(resolve_legacy_profile_name "${req}")"
    if [[ -n "${legacy}" ]]; then
      PROFILES+=("${legacy%% *}")
    else
      PROFILES+=("${req}")
    fi
  done
else
  while IFS= read -r p; do
    [[ -n "${p}" ]] && PROFILES+=("${p}")
  done < <(discover_profiles "${ROOT}")
fi

compute_cache_key() {
  local ext_file="${1}"
  local ext_sha pins_sha
  ext_sha="$(sha256_for_file "${ext_file}")" || return 1
  if [[ -f "${PINS_FILE}" ]]; then
    pins_sha="$(sha256_for_file "${PINS_FILE}")" || return 1
  else
    pins_sha="none"
  fi
  printf '%s|%s|%s|%s' "${ext_sha}" "${pins_sha}" "${CODE_USER_DIR}" "${EXCLUDE_GROUPS[*]+"${EXCLUDE_GROUPS[*]}"}" | sha256_for_file -
}

sync_profile_settings() {
  local profile="${1}"
  local merged="${ROOT}/_merged/${profile}.json"
  local tmp_merged=""
  if [[ ! -f "${merged}" ]]; then
    if [[ -f "${ROOT}/profiles/${profile}/profile.json" ]]; then
      tmp_merged="$(mktemp "${TMPDIR:-/tmp}/vsp-merged.XXXXXX")" || return 1
      if ! compiler_compile_profile_settings_file "${ROOT}" "${profile}" "${tmp_merged}"; then
        rm -f "${tmp_merged}"
        return 1
      fi
      merged="${tmp_merged}"
    else
      return 0
    fi
  fi

  local prof_id=""
  prof_id="$(vscode_storage_find_profile_id "${CODE_USER_DIR}" "${profile}")" || {
    printf 'Warning: could not read profile registration for %s\n' "${profile}" >&2
    [[ -n "${tmp_merged}" ]] && rm -f "${tmp_merged}"
    return 0
  }
  if [[ -z "${prof_id}" ]]; then
    printf 'Warning: profile %s is not registered in storage.json; skipping settings sync\n' "${profile}" >&2
    [[ -n "${tmp_merged}" ]] && rm -f "${tmp_merged}"
    return 0
  fi

  local dest="${CODE_USER_DIR}/profiles/${prof_id}/settings.json"
  mkdir -p "$(dirname -- "${dest}")"
  local tmp_sync
  tmp_sync="$(mktemp "${TMPDIR:-/tmp}/vsp-sync.XXXXXX")" || { [[ -n "${tmp_merged}" ]] && rm -f "${tmp_merged}"; return 1; }

  if [[ -f "${dest}" ]]; then
    backup_user_file "${dest}" 5 || { rm -f "${tmp_sync}"; [[ -n "${tmp_merged}" ]] && rm -f "${tmp_merged}"; return 1; }
    # Preserve existing local 14-key UX settings while syncing _merged/<name>.json.
    local strip="${LIB_DIR}/jsonc-strip.awk"
    local existing_clean
    if existing_clean="$(LC_ALL=C awk -f "${strip}" "${dest}" 2>/dev/null)" \
       && printf '%s\n' "${existing_clean}" | jq -e -s 'length == 1 and (.[0] | type == "object")' >/dev/null 2>&1; then
      printf '%s\n' "${existing_clean}" \
        | jq -s --argjson ux_keys "${UX_KEYS_JSON}" '
            .[1] as $new
            | (.[0] | with_entries(select(.key as $k | $ux_keys | index($k) != null))) as $preserved_ux
            | $new * $preserved_ux
          ' - "${merged}" > "${tmp_sync}" || { rm -f "${tmp_sync}"; [[ -n "${tmp_merged}" ]] && rm -f "${tmp_merged}"; return 1; }
    else
      cp "${merged}" "${tmp_sync}"
    fi
  else
    cp "${merged}" "${tmp_sync}"
  fi

  local rc=0
  atomic_write_file "${dest}" "${tmp_sync}" 0600 || rc=$?
  rm -f "${tmp_sync}"
  [[ -n "${tmp_merged}" ]] && rm -f "${tmp_merged}"
  return "${rc}"
}

FAILED=()
for profile in "${PROFILES[@]+"${PROFILES[@]}"}"; do
  if [[ "${SKIP_INSTALL}" != "1" ]]; then
    ext_file="${ROOT}/profiles/${profile}/extensions.json"
    tmp_ext_file=""
    if [[ ! -f "${ext_file}" && -f "${ROOT}/profiles/${profile}/profile.json" ]]; then
      tmp_ext_file="$(mktemp "${TMPDIR:-/tmp}/vsp-open-ext.XXXXXX")"
      if ! compiler_compile_profile_extensions_file "${ROOT}" "${profile}" "${tmp_ext_file}"; then
        rm -f "${tmp_ext_file}"
        FAILED+=("${profile}")
        continue
      fi
      ext_file="${tmp_ext_file}"
    fi
    if [[ -f "${ext_file}" ]]; then
      key="$(compute_cache_key "${ext_file}")" || { [[ -n "${tmp_ext_file}" ]] && rm -f "${tmp_ext_file}"; exit 1; }
      [[ -n "${tmp_ext_file}" ]] && rm -f "${tmp_ext_file}"
      marker="${CACHE_DIR}/${profile}"
      cached=""
      [[ -f "${marker}" ]] && cached="$(cat "${marker}" 2>/dev/null || true)"
      if [[ "${cached}" != "${key}" ]]; then
        inst_cmd=("${BASH}" "${SCRIPT_ROOT}/scripts/install-extensions.sh" "${profile}")
        for g in "${EXCLUDE_GROUPS[@]+"${EXCLUDE_GROUPS[@]}"}"; do
          inst_cmd+=(--without-group "${g}")
        done
        if "${inst_cmd[@]}"; then
          printf '%s\n' "${key}" > "${marker}"
        else
          FAILED+=("${profile}")
          continue
        fi
      fi
    fi
  fi

  sync_profile_settings "${profile}" || true

  code_args=()
  if [[ -n "${CODE_STORE_DATA_ROOT}" ]]; then
    code_args+=(--user-data-dir "${CODE_STORE_DATA_ROOT}")
  fi
  code_args+=(--profile "${profile}" --new-window)
  if ! code "${code_args[@]}"; then
    FAILED+=("${profile}")
  fi
done

if [[ "${#FAILED[@]}" -gt 0 ]]; then
  printf 'open-profiles: failed for: %s\n' "${FAILED[*]}" >&2
  exit 1
fi
exit 0
