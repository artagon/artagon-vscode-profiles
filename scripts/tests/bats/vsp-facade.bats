#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

setup() {
  setup_hermetic_env
  unset VSP_BIN
  export VSP_FORCE_BASH=1
}

teardown() {
  unset VSP_BIN VSP_FORCE_BASH XDG_BIN_HOME
  if [[ -n "${TEST_TMP:-}" ]] && [[ "${TEST_TMP}" != "${BATS_TEST_TMPDIR:-}" ]]; then
    rm -rf "${TEST_TMP}"
  fi
  export PATH="${REAL_ORIG_PATH}"
}

vsp() {
  "${REPO_ROOT}/bin/vsp" "${@+"${@}"}"
}

@test "facade: bin/vsp is executable and strictly <= 400 LOC" {
  [[ -x "${REPO_ROOT}/bin/vsp" ]]
  loc="$(wc -l < "${REPO_ROOT}/bin/vsp" | tr -d ' ')"
  [[ "${loc}" -le 400 ]]
}

@test "facade: relative symlink chain resolves REPO_ROOT for --help, --version, and self path" {
  hop_dir="${TEST_TMP}/hop dir"
  bin_dir="${TEST_TMP}/my bin"
  mkdir -p "${hop_dir}" "${bin_dir}"
  hop_phys="$(CDPATH='' cd -P -- "${hop_dir}" && pwd -P)"
  repo_phys="$(CDPATH='' cd -P -- "${REPO_ROOT}" && pwd -P)"
  up_prefix="$(printf '%s' "${hop_phys#/}" | awk -F/ '{for (i=1; i<=NF; i++) printf "../"}')"
  rel_target="${up_prefix}${repo_phys#/}/bin/vsp"
  ln -s "${rel_target}" "${hop_dir}/vsp-link"
  ln -s "../hop dir/vsp-link" "${bin_dir}/vsp"

  run env PATH="${bin_dir}:${PATH}" vsp --help
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"vsp — Manage Artagon VS Code"* ]]
  [[ "${output}" == *"sync"* ]]
  [[ "${output}" == *"diff"* ]]

  run env PATH="${bin_dir}:${PATH}" vsp --version
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"vsp 3.0.0"* ]]

  run env PATH="${bin_dir}:${PATH}" vsp self path
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == "${repo_phys}/bin/vsp" ]]
}

@test "facade: self install and self uninstall manage XDG_BIN_HOME/vsp and refuse non-symlink clobber" {
  export XDG_BIN_HOME="${TEST_TMP}/local-bin"
  repo_phys="$(CDPATH='' cd -P -- "${REPO_ROOT}" && pwd -P)"

  run vsp self install
  [[ "${status}" -eq 0 ]]
  [[ -L "${XDG_BIN_HOME}/vsp" ]]
  [[ "$(readlink "${XDG_BIN_HOME}/vsp")" == "${repo_phys}/bin/vsp" ]]

  run vsp self uninstall
  [[ "${status}" -eq 0 ]]
  [[ ! -e "${XDG_BIN_HOME}/vsp" && ! -L "${XDG_BIN_HOME}/vsp" ]]

  printf '#!/bin/sh\n' > "${XDG_BIN_HOME}/vsp"
  run vsp self install
  [[ "${status}" -eq 5 ]]

  run vsp self uninstall
  [[ "${status}" -eq 5 ]]
}

@test "facade: sync and diff with no selector exit 4 and never mutate cwd" {
  ws="$(make_workspace no-sel Cargo.toml)"

  run bash -c 'cd "$1" && "$2" sync' _ "${ws}" "${REPO_ROOT}/bin/vsp"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"explicit target selector required"* ]]
  [[ ! -d "${ws}/.vscode" ]]

  run bash -c 'cd "$1" && "$2" diff' _ "${ws}" "${REPO_ROOT}/bin/vsp"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"explicit target selector required"* ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "facade: detect and diff are strictly read-only and never create .vscode/" {
  ws="$(make_workspace rust-ro Cargo.toml)"

  run vsp detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"rust"* ]]
  [[ ! -d "${ws}/.vscode" ]]

  run vsp diff --toolchain rust --check "${ws}"
  [[ "${status}" -eq 3 ]]
  [[ "${output}" == *"DRY-RUN"* ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "facade: list, show, and dev docs --check succeed" {
  run vsp list
  [[ "${status}" -eq 0 ]]
  count="$(printf '%s\n' "${output}" | wc -l | tr -d ' ')"
  [[ "${count}" -eq 15 ]]
  [[ "${output}" == *"rust"* ]]
  [[ "${output}" == *"ai-copilot"* ]]

  run vsp show ai
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"chat.editor.fontSize"* ]]

  run vsp show nonexistent-profile-xyz
  [[ "${status}" -eq 2 ]]

  run vsp dev docs --check
  [[ "${status}" -eq 0 ]]
}

@test "facade: sync -p <name> runs Step 0 (reconcile --compromised-only) before settings write and extension installs" {
  mock_bin="${TEST_TMP}/step0-bin"
  install_code_mock "${mock_bin}"
  mv "${mock_bin}/code" "${mock_bin}/code.real"
  cat > "${mock_bin}/code" <<'WRAP'
#!/usr/bin/env bash
set -euo pipefail
settings_count=0
if [[ -d "${VSCODE_USER_DIR}/profiles" ]]; then
  settings_count="$(find "${VSCODE_USER_DIR}/profiles" -name settings.json 2>/dev/null | wc -l | tr -d ' ')"
fi
for arg in "$@"; do
  case "${arg}" in
    --uninstall-extension)
      echo "probe=uninstall settings_count=${settings_count}" >> "${CODE_LOG}"
      ;;
    --install-extension)
      echo "probe=install settings_count=${settings_count}" >> "${CODE_LOG}"
      ;;
  esac
done
exec "$(dirname -- "$0")/code.real" "$@"
WRAP
  chmod 0755 "${mock_bin}/code"

  data_root="${TEST_TMP}/vscode"
  # Seed compromised extension + all recipe extensions except the first so Step 2 installs 1 extension fast.
  seeded=("JScearcy.rust-doc-viewer")
  while IFS= read -r eid; do
    [[ -n "${eid}" ]] && seeded+=("${eid}")
  done < <("${BASH}" "${LIB_DIR}/compose-extensions.sh" general-minimal | jq -r '.[1:][].identifier.id')
  seed_code_mock_exts "general-minimal" "${data_root}" -- "${seeded[@]}"

  run vsp sync -p general-minimal --check-compat=off --no-rtk
  [[ "${status}" -eq 0 ]]

  # Step 0 must uninstall compromised extension BEFORE settings.json exists and BEFORE any install.
  first_probe="$(grep '^probe=' "${CODE_LOG}" | head -n 1)"
  [[ "${first_probe}" == "probe=uninstall settings_count=0" ]]

  first_mut="$(grep -E '^action=(install|uninstall) ' "${CODE_LOG}" | head -n 1)"
  [[ "${first_mut}" == *"action=uninstall profile=general-minimal"* ]]
  [[ "${first_mut}" == *"ext=jscearcy.rust-doc-viewer"* ]]

  # Step 1 wrote profile settings before Step 2 installs ran.
  grep -q '^probe=install settings_count=1$' "${CODE_LOG}"
}

@test "facade: VSP_BIN delegates directly with exact arguments and exit status" {
  stub="${TEST_TMP}/vsp-stub"
  cat > "${stub}" <<'STUB'
#!/usr/bin/env bash
printf 'STUB_CALLED argc=%d args=%s\n' "$#" "$*"
exit 7
STUB
  chmod 0755 "${stub}"

  run env VSP_BIN="${stub}" "${REPO_ROOT}/bin/vsp" doctor "--strict" "two words"
  [[ "${status}" -eq 7 ]]
  [[ "${output}" == "STUB_CALLED argc=3 args=doctor --strict two words" ]]
}

@test "facade: unknown commands and flags exit 4" {
  run vsp --unknown-flag
  [[ "${status}" -eq 4 ]]

  run vsp nonexistent-verb
  [[ "${status}" -eq 4 ]]

  run vsp dev nonexistent-sub
  [[ "${status}" -eq 4 ]]

  run vsp self nonexistent-sub
  [[ "${status}" -eq 4 ]]
}

@test "facade: bash-fallback diff --check never silently exits 0 on --user, --all, existing workspace, or partial profile checks" {
  install_code_mock "${TEST_TMP}/check-bin"

  run --separate-stderr vsp diff --check --user
  [[ "${status}" -eq 7 ]]
  [[ "${stderr}" == *"compiled Rust vsp binary is required"* ]]

  run --separate-stderr vsp diff --check --all
  [[ "${status}" -eq 7 ]]
  [[ "${stderr}" == *"compiled Rust vsp binary is required"* ]]

  run --separate-stderr vsp diff --check --profile rust --compromised-only
  [[ "${status}" -eq 7 ]]
  [[ "${stderr}" == *"compiled Rust vsp binary is required"* ]]

  # Workspace that already has .vscode/settings.json cannot be verified by Bash fallback alone -> exit 7 (not 0!).
  ws="$(make_workspace existing-ws Cargo.toml)"
  mkdir -p "${ws}/.vscode"
  printf '{"editor.tabSize": 99}\n' > "${ws}/.vscode/settings.json"
  run --separate-stderr vsp diff --toolchain rust --check "${ws}"
  [[ "${status}" -eq 7 ]]
  [[ "${stderr}" == *"compiled Rust vsp binary is required"* ]]

  # Profile with all extensions already installed still lacks settings drift verification in Bash fallback -> exit 7 (not 0!).
  data_root="${TEST_TMP}/vscode"
  seeded=()
  while IFS= read -r eid; do
    [[ -n "${eid}" ]] && seeded+=("${eid}")
  done < <("${BASH}" "${LIB_DIR}/compose-extensions.sh" general-minimal | jq -r '.[].identifier.id')
  seed_code_mock_exts "general-minimal" "${data_root}" -- "${seeded[@]}"
  run --separate-stderr vsp diff --profile general-minimal --check --check-compat=off --no-rtk
  [[ "${status}" -eq 7 ]]
  [[ "${stderr}" == *"compiled Rust vsp binary is required"* ]]
}

@test "facade: scripts/vspcli VSP_USE_RUST_SHIM=1 delegates with exact argv/exit/compat-env and refuses recursive targets" {
  stub="${TEST_TMP}/vsp-shim-stub"
  cat > "${stub}" <<'STUB'
#!/usr/bin/env bash
printf 'SHIM compat=%s argc=%d args=%s\n' "${VSP_COMPAT_VSPCLI:-unset}" "$#" "$*"
exit 9
STUB
  chmod 0755 "${stub}"

  run env VSP_USE_RUST_SHIM=1 VSP_BIN="${stub}" "${SCRIPTS_DIR}/vspcli" list "--flag" "two words"
  [[ "${status}" -eq 9 ]]
  [[ "${output}" == "SHIM compat=1 argc=3 args=list --flag two words" ]]

  # Recursion guard refuses direct and symlinked paths to scripts/vspcli or bin/vsp.
  link_vspcli="${TEST_TMP}/link-vspcli"
  link_facade="${TEST_TMP}/link-facade"
  ln -s "${SCRIPTS_DIR}/vspcli" "${link_vspcli}"
  ln -s "${REPO_ROOT}/bin/vsp" "${link_facade}"

  for bad_target in "${SCRIPTS_DIR}/vspcli" "${REPO_ROOT}/bin/vsp" "${link_vspcli}" "${link_facade}"; do
    run --separate-stderr env VSP_USE_RUST_SHIM=1 VSP_BIN="${bad_target}" "${SCRIPTS_DIR}/vspcli" list
    [[ "${status}" -eq 4 ]]
    [[ "${stderr}" == *"refusing recursive"* ]]
  done
}
