#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "cli: --help returns unified verb & flag help text" {
  run vspcli --help
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"vspcli"* ]]
  [[ "${output}" == *"build"* ]]
  [[ "${output}" == *"reconcile"* ]]
  [[ "${output}" == *"doctor"* ]]
  [[ "${output}" == *"--list"* ]]
}

@test "cli: list / --list lists all 15 compiled profile recipes" {
  run vspcli --list
  [[ "${status}" -eq 0 ]]
  count="$(printf '%s\n' "${output}" | wc -l | tr -d ' ')"
  [[ "${count}" -eq 15 ]]
  [[ "${output}" == *"rust"* ]]
  [[ "${output}" == *"astro"* ]]
  [[ "${output}" == *"general"* ]]
  [[ "${output}" == *"general-minimal"* ]]
  [[ "${output}" == *"ai-copilot"* ]]
  [[ "${output}" == *"python"* ]]
  [[ "${output}" == *"shell"* ]]
}

@test "cli: --completion bash emits a snippet" {
  run vspcli --completion bash
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"_vspcli_completions"* ]]
  [[ "${output}" == *"complete -F"* ]]
}

@test "cli: --completion zsh emits a snippet including ai-copilot and general-minimal" {
  run vspcli --completion zsh
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"#compdef vspcli"* ]]
  [[ "${output}" == *"ai-copilot"* ]]
  [[ "${output}" == *"general-minimal"* ]]
}

@test "cli: --completion fish emits a snippet including unified verbs" {
  run vspcli --completion fish
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"complete -c vspcli"* ]]
  [[ "${output}" == *"reconcile"* ]]
}

@test "cli: --completion powershell exits non-zero" {
  run vspcli --completion powershell
  [[ "${status}" -ne 0 ]]
}

@test "cli: --detect defaults to workspace installation" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli --detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"rust"* ]]
  [[ -f "${ws}/.vscode/extensions.json" ]]
  [[ -f "${ws}/.vscode/settings.json" ]]
}

@test "cli: --migrate-catalog --doctor returns 0 when clean" {
  run vspcli --migrate-catalog --doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"15 profiles present"* ]]
}

@test "cli: unknown flag exits non-zero" {
  run vspcli --xx-bogus-flag
  [[ "${status}" -eq 4 ]]
}

@test "cli: --detect --target=workspace --dry-run does not create files" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli --detect --target=workspace --dry-run "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "cli: --detect --target=invalid exits 4" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli --detect --target=zzz "${ws}"
  [[ "${status}" -eq 4 ]]
}

@test "cli: duplicate targets are rejected" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli --detect --target=workspace --target=global "${ws}"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"--target accepts only one"* ]]
}

@test "cli: migration cannot be combined with other actions" {
  run vspcli --migrate-catalog --detect
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"--migrate-catalog cannot be combined with: --detect"* ]]
}

@test "cli: relative symlink chain on PATH resolves scripts/lib and exits 0 on --help" {
  hop_dir="${TEST_TMP}/hop dir"
  bin_dir="${TEST_TMP}/my bin"
  mkdir -p "${hop_dir}" "${bin_dir}"
  hop_phys="$(CDPATH='' cd -P -- "${hop_dir}" && pwd -P)"
  repo_phys="$(CDPATH='' cd -P -- "${REPO_ROOT}" && pwd -P)"
  up_prefix="$(printf '%s' "${hop_phys#/}" | awk -F/ '{for (i=1; i<=NF; i++) printf "../"}')"
  rel_target="${up_prefix}${repo_phys#/}/scripts/vspcli"
  ln -s "${rel_target}" "${hop_dir}/vspcli-link"
  ln -s "../hop dir/vspcli-link" "${bin_dir}/vspcli"

  run env PATH="${bin_dir}:${PATH}" vspcli --help
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"vspcli — Manage Artagon VS Code"* ]]
  [[ "${output}" == *"reconcile"* ]]
}

@test "cli: legacy --detect emits deprecation warning on stderr while preserving behavior and exit code" {
  ws="$(make_workspace rust-dep Cargo.toml)"
  run --separate-stderr vspcli --detect --check-compat=off "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"deprecated"* ]]
  [[ "${stderr}" == *"vspcli detect"* ]]
  [[ "${stderr}" == *"vspcli apply"* ]]
  [[ -f "${ws}/.vscode/extensions.json" ]]
  [[ -f "${ws}/.vscode/settings.json" ]]
}

@test "cli: detect propagates module discovery failure instead of exiting 3 (no toolchain detected)" {
  bad_repo="${TEST_TMP}/bad-detect-repo"
  mkdir -p "${bad_repo}/modules/toolchains/rust"
  printf '{broken json\n' > "${bad_repo}/modules/toolchains/rust/module.json"
  ws="$(make_workspace rust-bad-cat Cargo.toml)"

  run env VSPCLI_ROOT="${bad_repo}" "${BASH}" "${SCRIPTS_DIR}/detect-toolchain.sh" "${ws}"
  [[ "${status}" -ne 0 ]]
  [[ "${status}" -ne 3 ]]
}
