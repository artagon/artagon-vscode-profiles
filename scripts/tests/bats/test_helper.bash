#!/usr/bin/env bash
# test_helper.bash — Hermetic offline shared helpers for BATS tests under scripts/tests/bats/.
# Loaded via `load test_helper` from each .bats file.

REPO_ROOT="$(CDPATH='' cd -- "$(dirname -- "${BATS_TEST_FILENAME}")/../../.." && pwd -P)"
SCRIPTS_DIR="${REPO_ROOT}/scripts"
LIB_DIR="${SCRIPTS_DIR}/lib"
export REPO_ROOT SCRIPTS_DIR LIB_DIR

# Preserve real host Cargo/Rustup dirs before setup() isolates HOME.
REAL_CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo}"
REAL_RUSTUP_HOME="${RUSTUP_HOME:-${HOME}/.rustup}"
REAL_ORIG_PATH="${PATH}"
export REAL_CARGO_HOME REAL_RUSTUP_HOME REAL_ORIG_PATH

# Hermetic offline isolation helper — call from custom setup() overrides too.
setup_hermetic_env() {
  TEST_TMP="${BATS_TEST_TMPDIR:-$(mktemp -d)}"
  export TEST_TMP

  export CARGO_HOME="${REAL_CARGO_HOME}"
  export RUSTUP_HOME="${REAL_RUSTUP_HOME}"

  export HOME="${TEST_TMP}/home"
  export XDG_CONFIG_HOME="${TEST_TMP}/xdg-config"
  export XDG_STATE_HOME="${TEST_TMP}/xdg-state"
  export VSCODE_USER_DIR="${TEST_TMP}/vscode/User"
  export VSCODE_EXT_CACHE_DIR="${TEST_TMP}/ext-cache"
  export VSCODE_VSIX_CACHE_DIR="${TEST_TMP}/vsix-cache"
  export ARTAGON_VSCODE_TEST_RUNNING=0
  export VSCODE_EXTENSION_INSTALL_DELAY=0

  mkdir -p "${HOME}" "${XDG_CONFIG_HOME}" "${XDG_STATE_HOME}" \
    "${VSCODE_USER_DIR}/globalStorage" "${VSCODE_EXT_CACHE_DIR}" "${VSCODE_VSIX_CACHE_DIR}"

  # Empty pin manifest by default so offline tests never hit live Marketplace URLs.
  export ARTAGON_VSCODE_PINS_FILE="${TEST_TMP}/empty-pins.json"
  printf '{"version":1,"pins":{}}\n' > "${ARTAGON_VSCODE_PINS_FILE}"

  # Fail-closed curl stub + lightweight default offline code stub on PATH so
  # standalone `bats` never hits live Marketplace or launches real host VS Code.
  OFFLINE_BIN="${TEST_TMP}/offline-bin"
  mkdir -p "${OFFLINE_BIN}"
  cat > "${OFFLINE_BIN}/curl" <<'STUB'
#!/bin/sh
echo "ERROR: unexpected live curl call in offline test: $*" >&2
exit 99
STUB
  cat > "${OFFLINE_BIN}/code" <<'CODESTUB'
#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  printf '1.147.0\n0000000\narm64\n'
  exit 0
fi
exit 0
CODESTUB
  chmod 0755 "${OFFLINE_BIN}/curl" "${OFFLINE_BIN}/code"
  export PATH="${OFFLINE_BIN}:${REAL_ORIG_PATH}"

  # Clear leakage variables between tests.
  unset ARTAGON_VSCODE_DEPRECATION_SEEN ARTAGON_VSCODE_DEPRECATION_ACK \
        ARTAGON_VSCODE_DEPRECATION_SUMMARY ARTAGON_VSCODE_DEPRECATION_COUNT \
        ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER ARTAGON_VSCODE_BYPASS_PINS \
        ARTAGON_VSCODE_ALLOW_TEST_PIN_URL VSCODE_USER_DATA_DIR FAILING_EXTS \
        VSPCLI_ROOT CODE_MOCK_STATE_DIR CODE_LOG
}

setup() {
  setup_hermetic_env
}

teardown() {
  unset ARTAGON_VSCODE_DEPRECATION_SEEN ARTAGON_VSCODE_DEPRECATION_ACK \
        ARTAGON_VSCODE_DEPRECATION_SUMMARY ARTAGON_VSCODE_DEPRECATION_COUNT \
        ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER ARTAGON_VSCODE_BYPASS_PINS \
        ARTAGON_VSCODE_ALLOW_TEST_PIN_URL ARTAGON_VSCODE_PINS_FILE \
        FAILING_EXTS VSCODE_USER_DIR VSCODE_USER_DATA_DIR VSPCLI_ROOT \
        VSCODE_VSIX_CACHE_DIR VSCODE_BATCH_INSTALL \
        CODE_MOCK_STATE_DIR CODE_LOG
  export PATH="${REAL_ORIG_PATH}"
  if [[ -n "${TEST_TMP:-}" ]] && [[ "${TEST_TMP}" != "${BATS_TEST_TMPDIR:-}" ]]; then
    rm -rf "${TEST_TMP}"
  fi
}

# install_code_mock <bin_dir>
# Drops a stateful `code` mock into <bin_dir> and prepends it to PATH.
# Supports: --user-data-dir, --profile, --list-extensions, --show-versions,
#           --install-extension, --uninstall-extension, --new-window, --version.
install_code_mock() {
  local bin_dir="${1}"
  mkdir -p "${bin_dir}"
  : "${CODE_LOG:=${TEST_TMP}/code.log}"
  : "${CODE_MOCK_STATE_DIR:=${TEST_TMP}/code-mock-state}"
  mkdir -p "${CODE_MOCK_STATE_DIR}"
  export CODE_LOG CODE_MOCK_STATE_DIR
  cat > "${bin_dir}/code" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
profile="default"
user_data_dir=""
action="open"
ext=""
install_exts=()
show_versions=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)             printf '1.147.0\n0000000\narm64\n'; exit 0 ;;
    --user-data-dir)       user_data_dir="$2"; shift 2 ;;
    --user-data-dir=*)     user_data_dir="${1#*=}"; shift ;;
    --profile)             profile="$2"; shift 2 ;;
    --profile=*)           profile="${1#*=}"; shift ;;
    --list-extensions)     action="list"; shift ;;
    --show-versions)       show_versions=1; shift ;;
    --install-extension)   action="install"; install_exts+=("$2"); shift 2 ;;
    --uninstall-extension) action="uninstall"; ext="$2"; shift 2 ;;
    --new-window)          shift ;;
    *)                     shift ;;
  esac
done

state_dir="${CODE_MOCK_STATE_DIR:-/tmp/code-mock-state}"
mkdir -p "${state_dir}"
store_tag="default-store"
if [[ -n "${user_data_dir}" ]]; then
  store_tag="$(printf '%s' "${user_data_dir}" | tr '/: ' '___')"
fi
prof_tag="$(printf '%s' "${profile}" | tr '/: ' '___')"
state_file="${state_dir}/${store_tag}__${prof_tag}.txt"
log_file="${CODE_LOG:-/tmp/code.log}"
udd_log=""
[[ -n "${user_data_dir}" ]] && udd_log=" user_data_dir=${user_data_dir}"

case "${action}" in
  list)
    echo "action=list profile=${profile}${udd_log}" >> "${log_file}"
    if [[ -f "${state_file}" ]]; then
      while IFS= read -r line; do
        [[ -n "${line}" ]] || continue
        if [[ "${show_versions}" -eq 1 ]]; then
          case "${line}" in
            *@*) printf '%s\n' "${line}" ;;
            *)   printf '%s@1.0.0\n' "${line}" ;;
          esac
        else
          printf '%s\n' "${line%%@*}"
        fi
      done < "${state_file}"
    fi
    exit 0
    ;;
  install)
    fail_list="${FAILING_EXTS:-}"
    fail_list="${fail_list//,/ }"
    had_fail=0
    echo "action=install_call count=${#install_exts[@]} profile=${profile}${udd_log}" >> "${log_file}"
    for one_ext in "${install_exts[@]+"${install_exts[@]}"}"; do
      base="$(basename -- "${one_ext}")"
      ext_id="${base%.vsix}"
      ext_id_lc="$(printf '%s' "${ext_id}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
      matched_fail=0
      for f in ${fail_list}; do
        f_lc="$(printf '%s' "${f}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
        if [[ "${f_lc}" == "${ext_id_lc}" ]]; then
          echo "action=install_failed profile=${profile}${udd_log} ext=${ext_id} target=${one_ext}" >> "${log_file}"
          matched_fail=1
          had_fail=1
          break
        fi
      done
      if [[ "${matched_fail}" -eq 0 ]]; then
        echo "action=install profile=${profile}${udd_log} ext=${ext_id} target=${one_ext}" >> "${log_file}"
        touch "${state_file}"
        if ! grep -iq "^${ext_id_lc}\(@\|$\)" "${state_file}" 2>/dev/null; then
          printf '%s\n' "${ext_id}" >> "${state_file}"
        fi
      fi
    done
    exit "${had_fail}"
    ;;
  uninstall)
    ext_lc="$(printf '%s' "${ext}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
    echo "action=uninstall profile=${profile}${udd_log} ext=${ext}" >> "${log_file}"
    if [[ -f "${state_file}" ]]; then
      tmp="${state_file}.tmp.$$"
      : > "${tmp}"
      while IFS= read -r line; do
        [[ -n "${line}" ]] || continue
        lid="$(printf '%s' "${line%%@*}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
        [[ "${lid}" == "${ext_lc}" ]] && continue
        printf '%s\n' "${line}" >> "${tmp}"
      done < "${state_file}"
      mv -f "${tmp}" "${state_file}"
    fi
    exit 0
    ;;
  *)
    echo "action=open profile=${profile}${udd_log}" >> "${log_file}"
    exit 0
    ;;
esac
MOCK
  chmod 0755 "${bin_dir}/code"
  export PATH="${bin_dir}:${PATH}"
}

# Seed installed extensions for a profile in the stateful `code` mock.
# Usage: seed_code_mock_exts <profile> [user_data_dir] -- <ext1[@ver]> ...
seed_code_mock_exts() {
  local profile="${1}"; shift
  local udd=""
  if [[ "${1:-}" != "--" ]]; then
    udd="${1}"; shift
  fi
  [[ "${1:-}" == "--" ]] && shift
  local store_tag="default-store"
  [[ -n "${udd}" ]] && store_tag="$(printf '%s' "${udd}" | tr '/: ' '___')"
  local prof_tag
  prof_tag="$(printf '%s' "${profile}" | tr '/: ' '___')"
  mkdir -p "${CODE_MOCK_STATE_DIR}"
  local sf="${CODE_MOCK_STATE_DIR}/${store_tag}__${prof_tag}.txt"
  : > "${sf}"
  local item
  for item in "${@+"${@}"}"; do
    printf '%s\n' "${item}" >> "${sf}"
  done
}

# make_bundle <out_path> [setting_pairs...] -- [extension_ids...]
make_bundle() {
  local out="${1}"; shift
  local in_settings=1
  local settings_json="{}"
  local exts_json="[]"
  local pairs=() ids=()
  local arg
  for arg in "${@+"${@}"}"; do
    if [[ "${arg}" == "--" ]]; then
      in_settings=0; continue
    fi
    if (( in_settings )); then
      pairs+=("${arg}")
    else
      ids+=("${arg}")
    fi
  done
  if (( ${#pairs[@]} > 0 )); then
    settings_json="$(jq -n '{}')"
    local p k v
    for p in "${pairs[@]}"; do
      k="${p%%=*}"
      v="${p#*=}"
      settings_json="$(jq --arg k "${k}" --arg v "${v}" '.[$k] = $v' <<<"${settings_json}")"
    done
  fi
  if (( ${#ids[@]} > 0 )); then
    exts_json="$(printf '%s\n' "${ids[@]}" | jq -R . | jq -s .)"
  fi
  jq -n --argjson s "${settings_json}" --argjson e "${exts_json}" \
    '{settings: $s, extensions: {enabled: $e}}' > "${out}"
}

import_profile() {
  "${BASH}" "${SCRIPTS_DIR}/import-profile.sh" "${@+"${@}"}"
}

make_workspace() {
  local name="${1:-ws}"; shift || true
  local dir="${TEST_TMP}/${name}"
  mkdir -p "${dir}"
  local spec f content
  for spec in "${@+"${@}"}"; do
    case "${spec}" in
      *=*)
        f="${spec%%=*}"
        content="${spec#*=}"
        mkdir -p "${dir}/$(dirname -- "${f}")"
        printf '%s' "${content}" > "${dir}/${f}"
        ;;
      *)
        mkdir -p "${dir}/$(dirname -- "${spec}")"
        : > "${dir}/${spec}"
        ;;
    esac
  done
  printf '%s\n' "${dir}"
}

have_bin() {
  command -v "${1}" >/dev/null 2>&1
}

require_bin() {
  if ! have_bin "${1}"; then
    skip "${1} not on PATH"
  fi
}

detect() {
  "${BASH}" "${SCRIPTS_DIR}/detect-toolchain.sh" "${@+"${@}"}"
}

vspcli() {
  "${BASH}" "${SCRIPTS_DIR}/vspcli" "${@+"${@}"}"
}
