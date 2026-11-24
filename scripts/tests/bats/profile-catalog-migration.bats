#!/usr/bin/env bats
# bats file_tags=:no-parallel

bats_require_minimum_version 1.5.0
load test_helper

@test "legacy: rust-profile-crisp resolves to 'rust crisp'" {
  source "${LIB_DIR}/legacy-profile-name.sh"
  result="$(resolve_legacy_profile_name "rust-profile-crisp" 2>/dev/null)"
  [[ "${result}" = "rust crisp" ]]
}

@test "legacy: web-astro-retina resolves to 'astro retina'" {
  source "${LIB_DIR}/legacy-profile-name.sh"
  result="$(resolve_legacy_profile_name "web-astro-retina" 2>/dev/null)"
  [[ "${result}" = "astro retina" ]]
}

@test "legacy: ai-profile-crisp strictly resolves to 'ai-copilot crisp' (never upgraded to AI trio)" {
  source "${LIB_DIR}/legacy-profile-name.sh"
  result="$(resolve_legacy_profile_name "ai-profile-crisp" 2>/dev/null)"
  [[ "${result}" = "ai-copilot crisp" ]]
  result2="$(resolve_legacy_profile_name "ai-profile-retina" 2>/dev/null)"
  [[ "${result2}" = "ai-copilot retina" ]]
}

@test "legacy: java-profile aliases to java-maven" {
  source "${LIB_DIR}/legacy-profile-name.sh"
  result="$(resolve_legacy_profile_name "java-profile-crisp" 2>/dev/null)"
  [[ "${result}" = "java-maven crisp" ]]
}

@test "legacy: non-legacy name returns empty" {
  source "${LIB_DIR}/legacy-profile-name.sh"
  result="$(resolve_legacy_profile_name "rust" 2>/dev/null)"
  [[ -z "${result}" ]]
}

@test "legacy: deprecation warning emitted on first invocation" {
  unset ARTAGON_VSCODE_DEPRECATION_SEEN
  source "${LIB_DIR}/legacy-profile-name.sh"
  run --separate-stderr resolve_legacy_profile_name "rust-profile-crisp"
  [[ "${stderr}" == *"deprecation"* ]]
  [[ "${stderr}" == *"rust-profile-crisp"* ]]
}

@test "legacy: deprecation warning suppressed by ARTAGON_VSCODE_DEPRECATION_ACK" {
  export ARTAGON_VSCODE_DEPRECATION_ACK=1
  source "${LIB_DIR}/legacy-profile-name.sh"
  run --separate-stderr resolve_legacy_profile_name "rust-profile-crisp"
  [[ -z "${stderr}" ]]
  [[ "${output}" = "rust crisp" ]]
}

@test "legacy: warning suppressed on second invocation in same shell" {
  unset ARTAGON_VSCODE_DEPRECATION_SEEN
  source "${LIB_DIR}/legacy-profile-name.sh"
  resolve_legacy_profile_name "rust-profile-crisp" >/dev/null 2>&1
  run --separate-stderr resolve_legacy_profile_name "java-spring-retina"
  [[ -z "${stderr}" ]]
  [[ "${output}" = "java-spring retina" ]]
}

@test "migration: --doctor reports clean state for 15-recipe v2 catalog" {
  run "${BASH}" "${SCRIPTS_DIR}/migrate-catalog.sh" --doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"fully migrated"* ]]
  [[ "${output}" == *"15 profiles present"* ]]
}

@test "migration: profiles dir has 15 recipes" {
  count="$(find "${REPO_ROOT}/profiles" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
  [[ "${count}" -eq 15 ]]
}

@test "migration: legacy export symlinks exist and ai-profile-* strictly targets ai-copilot" {
  [[ "$(readlink "${REPO_ROOT}/exports/rust-profile-crisp.code-profile")" == "rust.code-profile" ]]
  [[ "$(readlink "${REPO_ROOT}/exports/ai-profile-crisp.code-profile")" == "ai-copilot.code-profile" ]]
  [[ "$(readlink "${REPO_ROOT}/exports/ai-profile-retina.code-profile")" == "ai-copilot.code-profile" ]]
}

@test "migration: --refresh-symlinks keeps ai-profile-* pointed at ai-copilot.code-profile" {
  fixture="${TEST_TMP}/refresh-repo"
  mkdir -p "${fixture}/exports"
  cp "${REPO_ROOT}/exports/ai-copilot.code-profile" "${fixture}/exports/"
  cp "${REPO_ROOT}/exports/ai.code-profile" "${fixture}/exports/"
  cp "${REPO_ROOT}/exports/rust.code-profile" "${fixture}/exports/"
  # Point ai-profile-crisp wrongly at ai.code-profile, then refresh.
  ln -sfn "ai.code-profile" "${fixture}/exports/ai-profile-crisp.code-profile"
  run env VSPCLI_ROOT="${fixture}" "${BASH}" "${SCRIPTS_DIR}/migrate-catalog.sh" --refresh-symlinks
  [[ "${status}" -eq 0 ]]
  [[ "$(readlink "${fixture}/exports/ai-profile-crisp.code-profile")" == "ai-copilot.code-profile" ]]
  [[ "$(readlink "${fixture}/exports/ai-profile-retina.code-profile")" == "ai-copilot.code-profile" ]]
  [[ "$(readlink "${fixture}/exports/rust-profile-crisp.code-profile")" == "rust.code-profile" ]]
}

@test "migration: self-contained legacy fixture collapses atomically without broken _merged.new symlink" {
  fixture="${TEST_TMP}/legacy-fixture"
  mkdir -p "${fixture}/_shared/ux" "${fixture}/_shared/extensions" \
           "${fixture}/_overrides" "${fixture}/profiles" "${fixture}/_merged" "${fixture}/exports"
  printf '{"telemetry.telemetryLevel":"off"}\n' > "${fixture}/_shared/editor-base.jsonc"
  printf '{"workbench.colorTheme":"Tokyo Night"}\n' > "${fixture}/_shared/ux/crisp.jsonc"
  printf '{"workbench.colorTheme":"Catppuccin Mocha"}\n' > "${fixture}/_shared/ux/retina.jsonc"
  printf '[{"identifier":{"id":"editorconfig.editorconfig"}}]\n' > "${fixture}/_shared/extensions/base.json"

  mkdir -p "${fixture}/profiles/rust-profile-crisp" "${fixture}/profiles/rust-profile-retina" \
           "${fixture}/profiles/ai-profile-crisp" "${fixture}/profiles/general"
  printf '[{"identifier":{"id":"rust-lang.rust-analyzer"}}]\n' > "${fixture}/profiles/rust-profile-crisp/extensions.json"
  printf '[{"identifier":{"id":"rust-lang.rust-analyzer"}}]\n' > "${fixture}/profiles/rust-profile-retina/extensions.json"
  printf '[{"identifier":{"id":"github.copilot-chat"}}]\n' > "${fixture}/profiles/ai-profile-crisp/extensions.json"
  printf '[{"identifier":{"id":"editorconfig.editorconfig"}}]\n' > "${fixture}/profiles/general/extensions.json"
  printf '{"rust-analyzer.check.command":"clippy"}\n' > "${fixture}/_overrides/rust-profile-crisp.jsonc"
  printf '{"rust-analyzer.check.command":"clippy"}\n' > "${fixture}/_overrides/rust-profile-retina.jsonc"

  run env VSPCLI_ROOT="${fixture}" "${BASH}" "${SCRIPTS_DIR}/migrate-catalog.sh"
  [[ "${status}" -eq 0 ]]
  [[ -d "${fixture}/profiles/rust" ]]
  [[ -d "${fixture}/profiles/ai-copilot" ]]
  [[ ! -e "${fixture}/profiles.new" ]]
  [[ ! -e "${fixture}/profiles.old" ]]
  [[ "$(readlink "${fixture}/profiles/rust/settings.json")" == "../../_merged/rust.json" ]]
  [[ -f "${fixture}/profiles/rust/settings.json" ]]
  [[ "$(readlink "${fixture}/exports/rust-profile-crisp.code-profile")" == "rust.code-profile" ]]
  [[ "$(readlink "${fixture}/exports/ai-profile-crisp.code-profile")" == "ai-copilot.code-profile" ]]
}

@test "migration: pre-existing profiles.new refuses migration and preserves staging dir intact" {
  fixture="${TEST_TMP}/partial-repo"
  mkdir -p "${fixture}/profiles.new"
  printf 'keep-me\n' > "${fixture}/profiles.new/sentinel.txt"

  run env VSPCLI_ROOT="${fixture}" "${BASH}" "${SCRIPTS_DIR}/migrate-catalog.sh"
  [[ "${status}" -eq 6 ]]
  [[ -f "${fixture}/profiles.new/sentinel.txt" ]]

  run env VSPCLI_ROOT="${fixture}" "${BASH}" "${SCRIPTS_DIR}/migrate-catalog.sh" --doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"partial migration state"* ]]
  [[ -f "${fixture}/profiles.new/sentinel.txt" ]]
}
