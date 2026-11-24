#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "compose: rust profile includes core-editor + AI suite (Antigravity default) + rust extensions" {
  require_bin jq
  run "${BASH}" "${LIB_DIR}/compose-extensions.sh" rust
  [[ "${status}" -eq 0 ]]
  ids="$(printf '%s\n' "${output}" | jq -r '.[].identifier.id')"
  grep -Fxq "rust-lang.rust-analyzer" <<<"${ids}"
  grep -Fxq "google.google-antigravity" <<<"${ids}"
  grep -Fxq "anthropic.claude-code" <<<"${ids}"
  grep -Fxq "openai.chatgpt" <<<"${ids}"
  grep -Fxq "github.copilot-chat" <<<"${ids}"
  ! grep -Fxqi "jscearcy.rust-doc-viewer" <<<"${ids}"
}

@test "compose: ai-copilot module contains ONLY github.copilot-chat (no Antigravity/Claude/Codex/legacy copilot)" {
  require_bin jq
  run jq -r '(.extensions.workspace + .extensions.profile)[]' "${REPO_ROOT}/modules/capabilities/ai-copilot/module.json"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == "github.copilot-chat" ]]
}

@test "compose: ai-agents module makes Google Antigravity default-first alongside Claude, Codex, and Copilot Chat" {
  require_bin jq
  run jq -r '.extensions.profile[0]' "${REPO_ROOT}/modules/capabilities/ai-agents/module.json"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == "google.google-antigravity" ]]
  run jq -r '.extensions.profile | sort | join(",")' "${REPO_ROOT}/modules/capabilities/ai-agents/module.json"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == "anthropic.claude-code,github.copilot-chat,google.google-antigravity,openai.chatgpt" ]]
}

@test "compose: rust toolchain module declares zero AI extensions" {
  require_bin jq
  run jq -r '(.extensions.workspace + .extensions.profile)[]' "${REPO_ROOT}/modules/toolchains/rust/module.json"
  [[ "${status}" -eq 0 ]]
  ! grep -qiE 'copilot|claude|chatgpt|antigravity|gemini' <<<"${output}"
}

@test "compose: dedup preserves first occurrence without duplicates" {
  require_bin jq
  run "${BASH}" "${LIB_DIR}/compose-extensions.sh" ai ai-plus
  [[ "${status}" -eq 0 ]]
  cnt="$(printf '%s\n' "${output}" | jq '[.[] | select(.identifier.id == "github.copilot-chat")] | length')"
  [[ "${cnt}" -eq 1 ]]
}

@test "compose: missing module/profile exits non-zero" {
  run "${BASH}" "${LIB_DIR}/compose-extensions.sh" nonexistent-flavor
  [[ "${status}" -ne 0 ]]
  [[ "${output}" == *"missing layer/module/profile"* ]]
}

@test "compose: all 15 compiled profiles/<name>/extensions.json match compose-extensions.sh" {
  require_bin jq
  for flavor in ai ai-copilot ai-plus astro cpp-clangd cpp-intellisense general general-minimal github-workflows java-gradle java-maven java-spring python rust shell; do
    profile_set="$(jq -S '[.[].identifier.id] | sort | unique' "${REPO_ROOT}/profiles/${flavor}/extensions.json")"
    composed_set="$("${BASH}" "${LIB_DIR}/compose-extensions.sh" "${flavor}" | jq -S '[.[].identifier.id] | sort | unique')"
    [[ "${profile_set}" = "${composed_set}" ]] || {
      echo "set mismatch for ${flavor}:" >&2
      diff -u <(printf '%s\n' "${profile_set}") <(printf '%s\n' "${composed_set}") >&2
      false
    }
  done
}

@test "install-extensions: 'all --target=global --dry-run' propagates target and dry-run to all 15 profiles" {
  install_code_mock "${TEST_TMP}/bin"
  run "${BASH}" "${SCRIPTS_DIR}/install-extensions.sh" all --target=global --dry-run --group AI
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"[DRY-RUN] target=global"* ]]
  [[ "${output}" == *"anthropic.claude-code"* ]]
  # Verify dry-run never invoked code CLI.
  [[ ! -f "${CODE_LOG}" ]]
}

@test "install-extensions: GROUP_ORDER includes DX group (rainbow-csv and dx-pro extensions never silently dropped)" {
  run --separate-stderr "${BASH}" "${SCRIPTS_DIR}/install-extensions.sh" general --dry-run
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"[DX] mechatroner.rainbow-csv"* ]]

  run --separate-stderr "${BASH}" "${SCRIPTS_DIR}/install-extensions.sh" ai-plus --dry-run --group DX
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" != *"unknown group"* ]]
  [[ "${output}" == *"[DX] alefragnani.bookmarks"* ]]
  [[ "${output}" == *"[DX] mechatroner.rainbow-csv"* ]]
}

@test "install-extensions: malformed, trailing-garbage, and multi-document extensions.json exit nonzero with 0 installs" {
  install_code_mock "${TEST_TMP}/bin"
  bad_repo="${TEST_TMP}/bad-ext-repo"
  mkdir -p "${bad_repo}/profiles/malformed" "${bad_repo}/profiles/trailing" "${bad_repo}/profiles/multidoc"
  cp -R "${REPO_ROOT}/catalog" "${bad_repo}/catalog"

  printf '{broken json\n' > "${bad_repo}/profiles/malformed/extensions.json"
  printf '[{"identifier":{"id":"enkia.tokyo-night"}}] trailing garbage\n' > "${bad_repo}/profiles/trailing/extensions.json"
  printf '[{"identifier":{"id":"enkia.tokyo-night"}}]\n[{"identifier":{"id":"editorconfig.editorconfig"}}]\n' > "${bad_repo}/profiles/multidoc/extensions.json"

  for prof in malformed trailing multidoc; do
    rm -f "${CODE_LOG}"
    run env VSPCLI_ROOT="${bad_repo}" "${BASH}" "${SCRIPTS_DIR}/install-extensions.sh" "${prof}"
    [[ "${status}" -ne 0 ]]
    [[ ! -s "${CODE_LOG}" ]]
  done
}
