#!/usr/bin/env bats
# bats file_tags=no-parallel

bats_require_minimum_version 1.5.0
load test_helper

setup() {
  setup_hermetic_env
  install_code_mock "${TEST_TMP}/bin"
}

@test "reconcile: --dry-run performs ZERO mutations including Step 0 compromised extensions" {
  data_root="${TEST_TMP}/vscode"
  seed_code_mock_exts "rust" "${data_root}" -- "JScearcy.rust-doc-viewer" "rust-lang.rust-analyzer"

  run vspcli reconcile --profile rust --dry-run
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"DRY-RUN"* ]]
  [[ "${output}" == *"[STEP 0] uninstall compromised: jscearcy.rust-doc-viewer"* ]]
  # Verify code mock recorded ONLY --list-extensions, ZERO installs or uninstalls!
  ! grep -q 'action=uninstall' "${CODE_LOG}"
  ! grep -q 'action=install' "${CODE_LOG}"
}

@test "reconcile: Step 0 uninstalls compromised extension UNCONDITIONALLY FIRST even when Step 1 installs fail" {
  data_root="${TEST_TMP}/vscode"
  seed_code_mock_exts "rust" "${data_root}" -- "jscearcy.rust-doc-viewer" "github.copilot"

  # Fail EVERY install in Step 1!
  FAILING_EXTS="rust-lang.rust-analyzer,google.google-antigravity,github.copilot-chat,anthropic.claude-code,openai.chatgpt" \
    run vspcli reconcile --profile rust
  [[ "${status}" -eq 1 ]]

  # First mutation in CODE_LOG MUST be uninstall of jscearcy.rust-doc-viewer!
  first_mut="$(grep -E 'action=(install|install_failed|uninstall)' "${CODE_LOG}" | head -n 1)"
  [[ "${first_mut}" == *"action=uninstall profile=rust user_data_dir=${data_root} ext=jscearcy.rust-doc-viewer"* ]]
  # Superseded github.copilot MUST be preserved because its replacement github.copilot-chat failed to install!
  ! grep -q 'action=uninstall.*ext=github.copilot$' "${CODE_LOG}"
}

@test "reconcile: --as ai-copilot --prune-unmanaged removes Antigravity + Claude + Codex + openai.codex-audio, but keeps codex-audio when openai.chatgpt stays desired" {
  data_root="${TEST_TMP}/vscode"
  # Seed ai-copilot with full AI suite + openai.codex-audio packMember + core extensions.
  seed_code_mock_exts "ai-copilot" "${data_root}" -- \
    "google.google-antigravity" "github.copilot-chat" "anthropic.claude-code" "openai.chatgpt" "openai.codex-audio" \
    "editorconfig.editorconfig" "tamasfe.even-better-toml" "dotenv.dotenv-vscode" "redhat.vscode-yaml" \
    "redhat.vscode-xml" "mechatroner.rainbow-csv" \
    "usernamehw.errorlens" "streetsidesoftware.code-spell-checker" "enkia.tokyo-night" \
    "pkief.material-icon-theme" "pkief.material-product-icons" \
    "catppuccin.catppuccin-vsc" "catppuccin.catppuccin-vsc-icons" "miguelsolorio.fluent-icons" \
    "yzhang.markdown-all-in-one" "bierner.markdown-mermaid" \
    "mads-hartmann.bash-ide-vscode" "timonwong.shellcheck" "mkhl.shfmt" \
    "bmalehorn.vscode-fish" "jeff-hykin.better-shellscript-syntax" "kherring.bats-test-runner"

  run vspcli reconcile --profile ai-copilot --as ai-copilot --prune-unmanaged
  [[ "${status}" -eq 0 ]]
  grep -q 'action=uninstall.*ext=google.google-antigravity' "${CODE_LOG}"
  grep -q 'action=uninstall.*ext=anthropic.claude-code' "${CODE_LOG}"
  grep -q 'action=uninstall.*ext=openai.chatgpt' "${CODE_LOG}"
  grep -q 'action=uninstall.*ext=openai.codex-audio' "${CODE_LOG}"

  # Now test profile 'ai' (where openai.chatgpt IS desired): --prune-unmanaged must NOT remove openai.codex-audio!
  : > "${CODE_LOG}"
  seed_code_mock_exts "ai" "${data_root}" -- \
    "google.google-antigravity" "github.copilot-chat" "anthropic.claude-code" "openai.chatgpt" "openai.codex-audio" \
    "editorconfig.editorconfig" "tamasfe.even-better-toml" "dotenv.dotenv-vscode" "redhat.vscode-yaml" \
    "redhat.vscode-xml" "mechatroner.rainbow-csv" \
    "usernamehw.errorlens" "streetsidesoftware.code-spell-checker" "enkia.tokyo-night" \
    "pkief.material-icon-theme" "pkief.material-product-icons" \
    "catppuccin.catppuccin-vsc" "catppuccin.catppuccin-vsc-icons" "miguelsolorio.fluent-icons" \
    "yzhang.markdown-all-in-one" "bierner.markdown-mermaid" \
    "mads-hartmann.bash-ide-vscode" "timonwong.shellcheck" "mkhl.shfmt" \
    "bmalehorn.vscode-fish" "jeff-hykin.better-shellscript-syntax" "kherring.bats-test-runner"

  run vspcli reconcile --profile ai --prune-unmanaged
  [[ "${status}" -eq 0 ]]
  ! grep -q 'action=uninstall.*ext=openai.codex-audio' "${CODE_LOG}"
}

@test "reconcile: dual-root resolve_code_store rejects mismatched or non-/User VSCODE_USER_DIR" {
  VSCODE_USER_DIR="${TEST_TMP}/not-ending-in-user" run vspcli reconcile --profile rust --dry-run
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"does not end in '/User'"* ]]

  VSCODE_USER_DATA_DIR="${TEST_TMP}/other-root" VSCODE_USER_DIR="${TEST_TMP}/vscode/User" \
    run vspcli reconcile --profile rust --dry-run
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"disagrees with VSCODE_USER_DATA_DIR/User"* ]]
}

@test "doctor: FAILs on compromised extension in Default profile (with exact uninstall command) and WARNs / --strict fails on application-scope & pin drift" {
  data_root="${TEST_TMP}/vscode"
  # Create Default settings with wrong telemetry & missing workspace trust, and seed compromised + drifted pin!
  printf '{"telemetry.telemetryLevel":"all"}\n' > "${VSCODE_USER_DIR}/settings.json"
  seed_code_mock_exts "default" "${data_root}" -- "JScearcy.rust-doc-viewer@4.2.1" "enkia.tokyo-night@0.9.0"

  pins="${TEST_TMP}/doc-pins.json"
  jq -n '{version:1, pins:{"enkia.tokyo-night":{version:"1.1.2", sha256:"0000000000000000000000000000000000000000000000000000000000000000", vsix_url:"https://marketplace.visualstudio.com/test.vsix"}}}' > "${pins}"

  ARTAGON_VSCODE_PINS_FILE="${pins}" run vspcli doctor
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"FAIL: SECURITY: compromised extension 'jscearcy.rust-doc-viewer' installed in profile 'default'"* ]]
  [[ "${output}" == *"--uninstall-extension jscearcy.rust-doc-viewer"* ]]
  [[ "${output}" == *"WARN: Default profile"*"telemetry.telemetryLevel=all"* ]]
  [[ "${output}" == *"WARN: Default profile"*"security.workspace.trust.untrustedFiles=MISSING"* ]]
  [[ "${output}" == *"WARN: profile 'default': pinned extension 'enkia.tokyo-night' version drift (installed=0.9.0, pinned=1.1.2)"* ]]

  # Remove compromised extension via reconcile --all --compromised-only, then doctor without --strict exits 0 while --strict exits 1 on warnings!
  run vspcli reconcile --all --compromised-only
  [[ "${status}" -eq 0 ]]

  ARTAGON_VSCODE_PINS_FILE="${pins}" run vspcli doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"0 fail(s)"* ]]

  ARTAGON_VSCODE_PINS_FILE="${pins}" run vspcli doctor --strict
  [[ "${status}" -eq 1 ]]
}

@test "doctor: failing code --list-extensions exits 1 with audit-incomplete FAIL while still scanning remaining profiles" {
  data_root="${TEST_TMP}/vscode"
  printf '{"userDataProfiles":[{"name":"broken-prof","location":"aaaa1111"},{"name":"other-prof","location":"bbbb2222"}]}\n' \
    > "${VSCODE_USER_DIR}/globalStorage/storage.json"
  seed_code_mock_exts "other-prof" "${data_root}" -- "JScearcy.rust-doc-viewer@4.2.1"

  fail_bin="${TEST_TMP}/fail-code-bin"
  mkdir -p "${fail_bin}"
  cat > "${fail_bin}/code" <<WRAP
#!/usr/bin/env bash
set -euo pipefail
want_fail=0
is_list=0
prev=""
for arg in "\$@"; do
  if [[ "\${prev}" == "--profile" && "\${arg}" == "broken-prof" ]]; then
    want_fail=1
  fi
  if [[ "\${arg}" == "--list-extensions" ]]; then
    is_list=1
  fi
  prev="\${arg}"
done
if [[ "\${want_fail}" -eq 1 && "\${is_list}" -eq 1 ]]; then
  echo "action=list_failed profile=broken-prof" >> "${CODE_LOG}"
  echo "profile database locked" >&2
  exit 1
fi
exec "${TEST_TMP}/bin/code" "\$@"
WRAP
  chmod 0755 "${fail_bin}/code"

  PATH="${fail_bin}:${PATH}" run vspcli doctor
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"FAIL: audit incomplete — failed to list extensions for profile 'broken-prof'"* ]]
  [[ "${output}" == *"profile database locked"* ]]
  [[ "${output}" == *"FAIL: SECURITY: compromised extension 'jscearcy.rust-doc-viewer' installed in profile 'other-prof'"* ]]
  grep -q 'action=list profile=default' "${CODE_LOG}"
  grep -q 'action=list_failed profile=broken-prof' "${CODE_LOG}"
  grep -q 'action=list profile=other-prof' "${CODE_LOG}"
}

@test "doctor: extensions.autoUpdate passes ONLY on boolean false; missing, true, onlyEnabledExtensions, and string false warn and fail --strict" {
  mkdir -p "${XDG_CONFIG_HOME}/artagon-vscode/rtk"
  : > "${XDG_CONFIG_HOME}/artagon-vscode/rtk/rtk-init.bash"
  : > "${XDG_CONFIG_HOME}/artagon-vscode/rtk/rtk-init.fish"
  : > "${XDG_CONFIG_HOME}/artagon-vscode/rtk/rtk-init.zsh"

  # 1. Missing extensions.autoUpdate -> WARN and fail --strict.
  printf '{"telemetry.telemetryLevel":"off","security.workspace.trust.untrustedFiles":"prompt"}\n' \
    > "${VSCODE_USER_DIR}/settings.json"
  run vspcli doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"WARN: Default profile"*"extensions.autoUpdate=MISSING"* ]]
  run vspcli doctor --strict
  [[ "${status}" -eq 1 ]]

  # 2. "onlyEnabledExtensions" -> WARN and fail --strict.
  printf '{"telemetry.telemetryLevel":"off","security.workspace.trust.untrustedFiles":"prompt","extensions.autoUpdate":"onlyEnabledExtensions"}\n' \
    > "${VSCODE_USER_DIR}/settings.json"
  run vspcli doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"WARN: Default profile"*"extensions.autoUpdate=\"onlyEnabledExtensions\""* ]]
  run vspcli doctor --strict
  [[ "${status}" -eq 1 ]]

  # 3. Boolean true -> WARN and fail --strict.
  printf '{"telemetry.telemetryLevel":"off","security.workspace.trust.untrustedFiles":"prompt","extensions.autoUpdate":true}\n' \
    > "${VSCODE_USER_DIR}/settings.json"
  run vspcli doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"WARN: Default profile"*"extensions.autoUpdate=true"* ]]
  run vspcli doctor --strict
  [[ "${status}" -eq 1 ]]

  # 4. String "false" (wrong JSON type) -> WARN and fail --strict.
  printf '{"telemetry.telemetryLevel":"off","security.workspace.trust.untrustedFiles":"prompt","extensions.autoUpdate":"false"}\n' \
    > "${VSCODE_USER_DIR}/settings.json"
  run vspcli doctor
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"WARN: Default profile"*"type=string"* ]]
  run vspcli doctor --strict
  [[ "${status}" -eq 1 ]]

  # 5. Exact boolean false -> PASS and --strict exits 0.
  printf '{"telemetry.telemetryLevel":"off","security.workspace.trust.untrustedFiles":"prompt","extensions.autoUpdate":false}\n' \
    > "${VSCODE_USER_DIR}/settings.json"
  run vspcli doctor --strict
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"PASS: Default profile extensions.autoUpdate == false"* ]]
  [[ "${output}" == *"0 fail(s), 0 warning(s)"* ]]
}
