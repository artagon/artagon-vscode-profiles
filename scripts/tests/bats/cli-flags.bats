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
