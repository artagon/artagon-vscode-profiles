#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

make_mini_valid_repo() {
  local dir="${1}"
  mkdir -p "${dir}/catalog" "${dir}/modules/capabilities/core-editor" "${dir}/modules/ux" "${dir}/profiles/general"
  cp "${REPO_ROOT}/catalog/extensions.json" "${dir}/catalog/"
  cp "${REPO_ROOT}/catalog/publishers.json" "${dir}/catalog/"
  cp "${REPO_ROOT}/catalog/defaults.json" "${dir}/catalog/"
  cp "${REPO_ROOT}/catalog/setting-scopes.json" "${dir}/catalog/"
  cp -R "${REPO_ROOT}/modules/capabilities" "${dir}/modules/"
  cp -R "${REPO_ROOT}/modules/toolchains" "${dir}/modules/"
  cp -R "${REPO_ROOT}/modules/ux" "${dir}/modules/"
  cp -R "${REPO_ROOT}/profiles" "${dir}/"
}

@test "validate-security: detects publisher allowlist drift, orphan active catalog row, and publisher prefix mismatch" {
  fix1="${TEST_TMP}/pub-drift"
  make_mini_valid_repo "${fix1}"
  # Add stray publisher to catalog/publishers.json
  jq '.publishers += ["zz-stray-pub"]' "${fix1}/catalog/publishers.json" > "${fix1}/catalog/publishers.json.tmp"
  mv -f "${fix1}/catalog/publishers.json.tmp" "${fix1}/catalog/publishers.json"
  run env VSPCLI_ROOT="${fix1}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"drifted from active module publishers"* ]]

  fix2="${TEST_TMP}/orphan-active"
  make_mini_valid_repo "${fix2}"
  jq '.extensions["orphan.unused-ext"] = {publisher:"orphan",group:"General",universal:false,status:"active"}' \
    "${fix2}/catalog/extensions.json" > "${fix2}/catalog/extensions.json.tmp"
  mv -f "${fix2}/catalog/extensions.json.tmp" "${fix2}/catalog/extensions.json"
  run env VSPCLI_ROOT="${fix2}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"orphan active catalog entry"* ]]

  fix3="${TEST_TMP}/prefix-mismatch"
  make_mini_valid_repo "${fix3}"
  jq '.extensions["enkia.tokyo-night"].publisher = "wrongpub"' \
    "${fix3}/catalog/extensions.json" > "${fix3}/catalog/extensions.json.tmp"
  mv -f "${fix3}/catalog/extensions.json.tmp" "${fix3}/catalog/extensions.json"
  run env VSPCLI_ROOT="${fix3}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"publisher prefix mismatch"* ]]
}

@test "validate-security: parsed-config autonomy deny-list rejects JSON bypass keys while ignoring prose comments" {
  source "${LIB_DIR}/catalog.sh"

  # Rejects chat.tools.terminal.autoApprove, bypassPermissions, claudeProcessWrapper
  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"chat.tools.terminal.autoApprove\":true}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"autonomy deny-list"* ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"defaultMode\":\"bypassPermissions\"}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"claudeCode.claudeProcessWrapper\":\"/tmp/x\"}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"antigravity.serverArgs\":[\"--evil\"]}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"antigravity.releaseBaseUrl\":\"https://evil.example\"}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"antigravity.caCertificates\":\"/tmp/rogue-ca.pem\"}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"chatgpt.cliExecutable\":\"/tmp/codex\"}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"geminicodeassist.agentYoloMode\":true}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 1 ]]

  # Clean settings JSON passes.
  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"editor.hover.enabled\":\"on\"}' | check_autonomy_denylist_json test.json"
  [[ "${status}" -eq 0 ]]
}

@test "validate-security: catalog/setting-scopes.json rejects wrong type, invalid enum, and machine-scoped key in workspace" {
  fix="${TEST_TMP}/bad-scopes"
  make_mini_valid_repo "${fix}"
  # Inject boolean editor.hover.enabled (wrong type) + machine-scoped python-envs.alwaysUseUv in workspace settings!
  ws_py="${fix}/modules/toolchains/python-base/settings.workspace.jsonc"
  jq '. + {"editor.hover.enabled": true, "python-envs.alwaysUseUv": true}' \
    <(LC_ALL=C awk -f "${LIB_DIR}/jsonc-strip.awk" "${ws_py}") \
    > "${ws_py}.tmp"
  mv -f "${ws_py}.tmp" "${ws_py}"

  run env VSPCLI_ROOT="${fix}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"editor.hover.enabled expected string, got boolean"* ]]
  [[ "${output}" == *"python-envs.alwaysUseUv has scope=machine and is forbidden in settings.workspace.jsonc"* ]]
}

@test "validate-security: pre-commit hook works without code CLI, allows staged-only M, and rejects T/??/unstaged M" {
  repo="${TEST_TMP}/git-hook-repo"
  mkdir -p "${repo}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" \
        "${REPO_ROOT}/_merged" "${REPO_ROOT}/exports" "${REPO_ROOT}/scripts" "${repo}/"

  git -C "${repo}" init -q
  git -C "${repo}" config user.email "test@example.com"
  git -C "${repo}" config user.name "Test User"
  git -C "${repo}" add -A
  git -C "${repo}" commit -q -m "baseline"

  # Strip 'code' from PATH to prove pre-commit does NOT require VS Code CLI!
  no_code_bin="${TEST_TMP}/no-code-bin"
  mkdir -p "${no_code_bin}"
  ln -s "${BASH}" "${no_code_bin}/bash"
  for tool in env git jq awk sed grep find sort diff tr cat cp mv rm mkdir chmod mktemp basename dirname readlink paste wc head tail stat shasum sha256sum openssl od dd date hostname ps pgrep kill sleep ln; do
    p="$(command -v "${tool}" 2>/dev/null || true)"
    [[ -n "${p}" ]] && ln -s "${p}" "${no_code_bin}/${tool}"
  done

  # Clean repo passes pre-commit even without `code`!
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 0 ]]

  # Untracked file under exports/ (??) must FAIL pre-commit!
  printf '{}\n' > "${repo}/exports/stray.json"
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"stray.json"* ]]
  rm -f "${repo}/exports/stray.json"

  # Symlink typechange (` T`) under profiles/ must FAIL pre-commit!
  rm -f "${repo}/profiles/ai/README.md"
  ln -s "../astro/README.md" "${repo}/profiles/ai/README.md"
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *" T profiles/ai/README.md"* ]]
  git -C "${repo}" checkout -- profiles/ai/README.md

  # Unstaged edit (` M`) must FAIL pre-commit!
  printf 'unstaged edit\n' >> "${repo}/profiles/ai/README.md"
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"profiles/ai/README.md"* ]]

  # Stage the edit (`M ` index-only, clean worktree) -> MUST PASS pre-commit!
  git -C "${repo}" add profiles/ai/README.md
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 0 ]]
}

@test "validate-security: symlink + non-existent parent traversal (x/../../../escaped) and dangling .bak symlink are blocked" {
  ws="$(make_workspace traversal-ws Cargo.toml)"
  mkdir -p "${ws}/.vscode"
  ln -s 'x/../../../escaped-settings.json' "${ws}/.vscode/settings.json"
  run vspcli apply --target=workspace --check-compat=off "${ws}"
  [[ "${status}" -eq 4 ]]
  [[ ! -e "${TEST_TMP}/escaped-settings.json" ]]

  # Dangling .bak symlink pointing outside workspace is refused when target has interior comments.
  ws2="$(make_workspace bak-ws Cargo.toml)"
  mkdir -p "${ws2}/.vscode"
  printf '{\n  // interior comment\n  "editor.tabSize": 4\n}\n' > "${ws2}/.vscode/settings.json"
  ln -s "${TEST_TMP}/outside-bak.json" "${ws2}/.vscode/settings.json.bak"
  run vspcli apply --target=workspace --allow-comment-loss --check-compat=off "${ws2}"
  [[ "${status}" -eq 4 ]]
  [[ ! -e "${TEST_TMP}/outside-bak.json" ]]
}

@test "validate-security: VSIX URL allowlist blocks @, ?, # spoofing and remote engines.vscode arithmetic injection" {
  source "${LIB_DIR}/vsix-pin.sh"
  unset ARTAGON_VSCODE_ALLOW_TEST_PIN_URL
  run _vsix_pin_validate_url 'https://marketplace.visualstudio.com:@evil.example/x.vsix'
  [[ "${status}" -ne 0 ]]
  run _vsix_pin_validate_url 'https://evil.example?.gallerycdn.vsassets.io/x.vsix'
  [[ "${status}" -ne 0 ]]
  run _vsix_pin_validate_url 'https://evil.example#.gallerycdn.vsassets.io/x.vsix'
  [[ "${status}" -ne 0 ]]
  run _vsix_pin_validate_url 'https://marketplace.visualstudio.com/_apis/public/gallery/publishers/enkia/vsextensions/tokyo-night/1.1.2/vspackage'
  [[ "${status}" -eq 0 ]]

  # Verify malicious engines.vscode string cannot execute shell commands via Bash arithmetic!
  marker="${TEST_TMP}/pwned-marker"
  resp="${TEST_TMP}/malicious-mp.json"
  cat > "${resp}" <<EOF
{"results":[{"extensions":[{"publisher":{"publisherName":"evil"},"extensionName":"arith","versions":[{"version":"1.0.0","properties":[{"key":"Microsoft.VisualStudio.Code.Engine","value":"^0+a[\$(touch '${marker}')]"}]}]}]}]}
EOF
  mal_bin="${TEST_TMP}/mal-bin"
  mkdir -p "${mal_bin}"
  cat > "${mal_bin}/curl" <<MOCK
#!/bin/sh
out=""
while [ \$# -gt 0 ]; do
  case "\$1" in -o) out="\$2"; shift 2 ;; *) shift ;; esac
done
cp '${resp}' "\${out}"
MOCK
  chmod 0755 "${mal_bin}/curl"
  ext_f="${TEST_TMP}/mal-ext.json"
  printf '[{"identifier":{"id":"evil.arith"}}]\n' > "${ext_f}"
  PATH="${mal_bin}:${PATH}" run "${BASH}" "${SCRIPTS_DIR}/check-extension-compatibility.sh" --extensions-file "${ext_f}" --vscode-version 1.147.0 --marketplace-only --json
  [[ "${status}" -eq 1 ]]
  [[ ! -e "${marker}" ]]
}

@test "validate-security: reconcile --all removes compromised extension from exact legacy profile name without rewriting" {
  install_code_mock "${TEST_TMP}/bin"
  data_root="${TEST_TMP}/vscode"
  printf '{"userDataProfiles":[{"name":"rust-profile-crisp","location":"aaaa1111"}]}\n' \
    > "${VSCODE_USER_DIR}/globalStorage/storage.json"
  seed_code_mock_exts "rust-profile-crisp" "${data_root}" -- "jscearcy.rust-doc-viewer"
  seed_code_mock_exts "default" "${data_root}" -- "jscearcy.rust-doc-viewer"

  run vspcli reconcile --all --compromised-only
  [[ "${status}" -eq 0 ]]
  grep -q 'action=uninstall profile=rust-profile-crisp .*ext=jscearcy.rust-doc-viewer' "${CODE_LOG}"
  grep -q 'action=uninstall profile=default .*ext=jscearcy.rust-doc-viewer' "${CODE_LOG}"
  ! grep -q 'action=uninstall profile=rust ' "${CODE_LOG}"
}
