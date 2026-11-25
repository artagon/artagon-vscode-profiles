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
  cp "${REPO_ROOT}/catalog/extension-pins.json" "${dir}/catalog/"
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

@test "validate-security: pre-commit hook works without code CLI, allows valid staged edits, and rejects staged generated artifacts" {
  repo="${TEST_TMP}/git-hook-repo"
  mkdir -p "${repo}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" \
        "${REPO_ROOT}/packs" "${REPO_ROOT}/bin" "${REPO_ROOT}/scripts" "${repo}/"
  cp "${REPO_ROOT}/presets.jsonc" "${REPO_ROOT}/vsp.lock.json" "${REPO_ROOT}/.gitignore" "${repo}/"

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

  # Staging a generated file under exports/ MUST FAIL pre-commit!
  mkdir -p "${repo}/exports"
  printf '{}\n' > "${repo}/exports/stray.code-profile"
  git -C "${repo}" add -f exports/stray.code-profile
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"exports/stray.code-profile"* ]]
  git -C "${repo}" reset -q HEAD -- exports/stray.code-profile
  rm -rf "${repo}/exports"

  # Staging a generated file under _merged/ MUST FAIL pre-commit!
  mkdir -p "${repo}/_merged"
  printf '{}\n' > "${repo}/_merged/rust.json"
  git -C "${repo}" add -f _merged/rust.json
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"_merged/rust.json"* ]]
  git -C "${repo}" reset -q HEAD -- _merged/rust.json
  rm -rf "${repo}/_merged"

  # Staging a generated profiles/*/extensions.json MUST FAIL pre-commit!
  printf '[]\n' > "${repo}/profiles/ai/extensions.json"
  git -C "${repo}" add -f profiles/ai/extensions.json
  run env PATH="${no_code_bin}" "${BASH}" "${repo}/scripts/git-hooks/pre-commit"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"profiles/ai/extensions.json"* ]]
  git -C "${repo}" reset -q HEAD -- profiles/ai/extensions.json
  rm -f "${repo}/profiles/ai/extensions.json"

  # Legitimate staged edit passes pre-commit!
  printf 'staged edit\n' >> "${repo}/profiles/ai/README.md"
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

@test "validate-security: H1/N1 autonomy deny-list rejects nested dot-paths, case-insensitive exec-overrides, and string booleans" {
  # All 5 required N1 payloads must exit nonzero.
  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"chat.tools.autoApprove\":true}' | check_autonomy_denylist_json n1-1.json"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"autonomy deny-list"* ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"chat\":{\"tools\":{\"autoApprove\":true}}}' | check_autonomy_denylist_json n1-2.json"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"chat.tools.autoApprove"* ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"chat\":{\"tools.terminal.autoApprove\":true}}' | check_autonomy_denylist_json n1-3.json"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"chat.tools.terminal.autoApprove"* ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"claudeCode\":{\"claudeProcessWrapper\":\"/tmp/x\"}}' | check_autonomy_denylist_json n1-4.json"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"claudeCode.claudeProcessWrapper"* ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"claudeCode.allowDangerouslySkipPermissions\":\"true\"}' | check_autonomy_denylist_json n1-5.json"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"claudeCode.allowDangerouslySkipPermissions"* ]]

  # Case-insensitive exec-override & nested string booleans also rejected; string "false" normalized safely.
  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"CLAUDECODE\":{\"ClaudeProcessWrapper\":\"/tmp/x\"}}' | check_autonomy_denylist_json n1-case.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"geminicodeassist\":{\"agentYoloMode\":\"TRUE\"}}' | check_autonomy_denylist_json n1-yolo.json"
  [[ "${status}" -eq 1 ]]

  run "${BASH}" -c "source '${LIB_DIR}/catalog.sh'; printf '%s\n' '{\"claudeCode.allowDangerouslySkipPermissions\":\"false\",\"geminicodeassist.agentYoloMode\":\"false\"}' | check_autonomy_denylist_json n1-false.json"
  [[ "${status}" -eq 0 ]]
}

@test "validate-security: H3 validate-json fails closed when catalog/extension-pins.json is absent" {
  fix="${TEST_TMP}/missing-pins-repo"
  make_mini_valid_repo "${fix}"

  # Control: valid mini repo with extension-pins.json passes.
  run env VSPCLI_ROOT="${fix}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 0 ]]

  # Removing catalog/extension-pins.json must fail nonzero (never silently skip).
  rm -f "${fix}/catalog/extension-pins.json"
  run env VSPCLI_ROOT="${fix}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"catalog/extension-pins.json"* ]]
}

@test "validate-security: H3 vsix-pin fails closed on missing/unreadable pins manifest unless ARTAGON_VSCODE_ALLOW_UNLOCKED=1" {
  missing_pins="${TEST_TMP}/no-such-pins.json"

  # Missing pins file fails closed by default (even when ARTAGON_VSCODE_PINS_FILE is unset).
  run --separate-stderr "${BASH}" -c "unset ARTAGON_VSCODE_PINS_FILE ARTAGON_VSCODE_ALLOW_UNLOCKED; source '${LIB_DIR}/vsix-pin.sh'; lookup_pin 'enkia.tokyo-night' '${missing_pins}'"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"missing or unreadable"* ]]

  # Explicit bypass ARTAGON_VSCODE_ALLOW_UNLOCKED=1 proceeds with a clear stderr warning.
  run --separate-stderr "${BASH}" -c "unset ARTAGON_VSCODE_PINS_FILE; export ARTAGON_VSCODE_ALLOW_UNLOCKED=1; source '${LIB_DIR}/vsix-pin.sh'; lookup_pin 'enkia.tokyo-night' '${missing_pins}'"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"WARNING"* && "${stderr}" == *"ARTAGON_VSCODE_ALLOW_UNLOCKED=1"* ]]

  # Existing valid empty-pins manifest still succeeds cleanly without warnings.
  empty_pins="${TEST_TMP}/valid-empty-pins.json"
  printf '{"version":1,"pins":{}}\n' > "${empty_pins}"
  run --separate-stderr "${BASH}" -c "unset ARTAGON_VSCODE_ALLOW_UNLOCKED; source '${LIB_DIR}/vsix-pin.sh'; lookup_pin 'enkia.tokyo-night' '${empty_pins}'"
  [[ "${status}" -eq 0 ]]
  [[ -z "${output}" ]]
  [[ -z "${stderr}" ]]

  # Unreadable pins manifest fails closed unless bypass is active (skip chmod check only if root).
  if [[ "$(id -u)" -ne 0 ]]; then
    unreadable_pins="${TEST_TMP}/unreadable-pins.json"
    printf '{"version":1,"pins":{}}\n' > "${unreadable_pins}"
    chmod 000 "${unreadable_pins}"
    run --separate-stderr "${BASH}" -c "unset ARTAGON_VSCODE_PINS_FILE ARTAGON_VSCODE_ALLOW_UNLOCKED; source '${LIB_DIR}/vsix-pin.sh'; lookup_pin 'enkia.tokyo-night' '${unreadable_pins}'"
    [[ "${status}" -ne 0 ]]
    [[ "${stderr}" == *"missing or unreadable"* ]]

    run --separate-stderr "${BASH}" -c "unset ARTAGON_VSCODE_PINS_FILE; export ARTAGON_VSCODE_ALLOW_UNLOCKED=1; source '${LIB_DIR}/vsix-pin.sh'; lookup_pin 'enkia.tokyo-night' '${unreadable_pins}'"
    [[ "${status}" -eq 0 ]]
    [[ "${stderr}" == *"WARNING"* && "${stderr}" == *"ARTAGON_VSCODE_ALLOW_UNLOCKED=1"* ]]
    chmod 0644 "${unreadable_pins}"
  fi
}

@test "validate-security: H3 vsix-pin propagates publish mv failure (cleaning staging temp) and sha256_for_file propagates digest failures" {
  vsix_payload="${TEST_TMP}/pub-fail.vsix"
  printf 'VERIFIED-PUBLISH-FAIL-PAYLOAD\n' > "${vsix_payload}"
  source "${LIB_DIR}/common.sh"
  good_sha="$(sha256_for_file "${vsix_payload}")"
  url="https://marketplace.visualstudio.com/_apis/public/gallery/publishers/enkia/vsextensions/tokyo-night/1.1.2/vspackage"

  pin_bin="${TEST_TMP}/pub-fail-bin"
  mkdir -p "${pin_bin}"
  cat > "${pin_bin}/curl" <<CURLMOCK
#!/bin/sh
out=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    -o) out="\$2"; shift 2 ;;
    *)  shift ;;
  esac
done
cp '${vsix_payload}' "\${out}"
CURLMOCK
  chmod 0755 "${pin_bin}/curl"

  # 1. Directory at out_path must fail closed and clean up staging temp (never move into directory).
  pub_dir="${TEST_TMP}/pub-target-parent"
  out_as_dir="${pub_dir}/target.vsix"
  mkdir -p "${out_as_dir}"

  PATH="${pin_bin}:${PATH}" ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1 VSCODE_VSIX_CACHE_DIR="" \
    run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/vsix-pin.sh'; fetch_and_verify_vsix 'enkia.tokyo-night' '${url}' '${good_sha}' '${out_as_dir}'"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"failed to publish"* ]]
  leftover="$(find "${pub_dir}" -name '.vsix-pin.*' -print)"
  [[ -z "${leftover}" ]]

  # 2. Explicit mv failure must propagate nonzero and clean up staging temp file.
  cat > "${pin_bin}/mv" <<'MVMOCK'
#!/bin/sh
exit 1
MVMOCK
  chmod 0755 "${pin_bin}/mv"
  out_file="${pub_dir}/real-target.vsix"
  PATH="${pin_bin}:${PATH}" ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1 VSCODE_VSIX_CACHE_DIR="" \
    run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/vsix-pin.sh'; fetch_and_verify_vsix 'enkia.tokyo-night' '${url}' '${good_sha}' '${out_file}'"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"failed to publish"* ]]
  [[ ! -e "${out_file}" ]]
  leftover="$(find "${pub_dir}" -name '.vsix-pin.*' -print)"
  [[ -z "${leftover}" ]]

  # common.sh sha256_for_file fails nonzero on missing file AND on failing sha256sum/shasum tool.
  run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; sha256_for_file '${TEST_TMP}/does-not-exist.bin'"
  [[ "${status}" -ne 0 ]]

  bad_hash_bin="${TEST_TMP}/bad-hash-bin"
  mkdir -p "${bad_hash_bin}"
  cat > "${bad_hash_bin}/sha256sum" <<'EOF'
#!/bin/sh
exit 1
EOF
  cat > "${bad_hash_bin}/shasum" <<'EOF'
#!/bin/sh
exit 1
EOF
  chmod 0755 "${bad_hash_bin}/sha256sum" "${bad_hash_bin}/shasum"
  PATH="${bad_hash_bin}:${PATH}" run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; sha256_for_file '${vsix_payload}'"
  [[ "${status}" -ne 0 ]]
}

@test "validate-security: H5 is_vscode_running matches spaced macOS paths and treats failed ps scan as busy (fail-closed)" {
  stub_bin="${TEST_TMP}/h5-ps-stub"
  mkdir -p "${stub_bin}"
  cat > "${stub_bin}/uname" <<'EOF'
#!/bin/sh
printf '%s\n' "${FAKE_UNAME:-Darwin}"
EOF
  cat > "${stub_bin}/ps" <<'EOF'
#!/bin/sh
if [ "${FAKE_PS_FAIL:-0}" = "1" ]; then
  exit 1
fi
printf '%s\n' "${FAKE_PS_OUT}"
EOF
  chmod 0755 "${stub_bin}/uname" "${stub_bin}/ps"

  # Spaced parent directory on macOS => detected (exit 0).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" \
    FAKE_PS_OUT="/Users/Jane Doe/Applications/Visual Studio Code.app/Contents/MacOS/Code" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 0 ]]

  # Spaced parent directory transient CLI out/cli.js => NOT detected (exit 1).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" \
    FAKE_PS_OUT="/Users/Jane Doe/Applications/Visual Studio Code.app/Contents/MacOS/Code /Users/Jane Doe/Applications/Visual Studio Code.app/Contents/Resources/app/out/cli.js --install-extension foo.bar" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 1 ]]

  # Unrelated command referencing spaced macOS VS Code path in argv[1+] => NOT detected (exit 1).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" \
    FAKE_PS_OUT="/usr/bin/codesign -dv /Users/Jane Doe/Applications/Visual Studio Code.app/Contents/MacOS/Code" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 1 ]]

  # Failed ps scan on macOS => busy/fail-closed (exit 0) with stderr warning.
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" FAKE_PS_FAIL="1" \
    run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"fail-closed"* ]]

  # Failed ps scan on Linux => busy/fail-closed (exit 0) with stderr warning.
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Linux" FAKE_PS_FAIL="1" \
    run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"fail-closed"* ]]
}

@test "validate-security: H5 acquire_lock serializes stale-lock reclamation without moving active owner locks" {
  local_host="$(hostname 2>/dev/null || printf 'unknown')"

  # Obtain a guaranteed confirmed-dead PID.
  "${BASH}" -c 'exit 0' &
  dead_pid=$!
  wait "${dead_pid}"

  # 1. Active owner lock is NOT moved/reclaimed even with --break-stale-lock.
  sleep 30 &
  live_pid=$!
  lock1="${TEST_TMP}/lock-active.lock"
  mkdir -p "${lock1}"
  printf '%s\n' "${live_pid}" > "${lock1}/pid"
  printf '%s\n' "${local_host}" > "${lock1}/host"
  printf 'sentinel\n' > "${lock1}/sentinel"

  run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; acquire_lock '${lock1}' 0 --break-stale-lock"
  kill "${live_pid}" 2>/dev/null || true
  wait "${live_pid}" 2>/dev/null || true
  [[ "${status}" -ne 0 ]]
  [[ -f "${lock1}/sentinel" ]]

  # 2. Pre-existing reclaim guard prevents another reclaimer from moving the lock (fails closed).
  lock2="${TEST_TMP}/lock-guarded.lock"
  mkdir -p "${lock2}" "${lock2}.reclaim"
  printf '%s\n' "${dead_pid}" > "${lock2}/pid"
  printf '%s\n' "${local_host}" > "${lock2}/host"
  printf 'guarded\n' > "${lock2}/sentinel"

  run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; acquire_lock '${lock2}' 0 --break-stale-lock"
  [[ "${status}" -ne 0 ]]
  [[ -f "${lock2}/sentinel" ]]
  rmdir "${lock2}.reclaim"

  # 3. Broken ps prevents false-dead reclamation.
  bad_ps_bin="${TEST_TMP}/bad-ps-bin"
  mkdir -p "${bad_ps_bin}"
  cat > "${bad_ps_bin}/ps" <<'EOF'
#!/bin/sh
exit 1
EOF
  chmod 0755 "${bad_ps_bin}/ps"
  PATH="${bad_ps_bin}:${PATH}" run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/common.sh'; acquire_lock '${lock2}' 0 --break-stale-lock"
  [[ "${status}" -ne 0 ]]
  [[ -f "${lock2}/sentinel" ]]

  # 4. Contested concurrent stale-lock reclamation maintains strict mutual exclusion and intact owner pid.
  lock3="${TEST_TMP}/lock-contested.lock"
  mkdir -p "${lock3}"
  printf '%s\n' "${dead_pid}" > "${lock3}/pid"
  printf '%s\n' "${local_host}" > "${lock3}/host"

  events_log="${TEST_TMP}/lock-events.log"
  err_log="${TEST_TMP}/lock-errors.log"
  : > "${events_log}"
  : > "${err_log}"

  pids=()
  for _w in 1 2 3 4; do
    "${BASH}" -c '
      source "'"${LIB_DIR}"'/common.sh"
      if ! acquire_lock "'"${lock3}"'" 15 --break-stale-lock; then
        printf "ACQUIRE_FAIL %s\n" "$$" >> "'"${err_log}"'"
        exit 1
      fi
      printf "ENTER %s\n" "$$" >> "'"${events_log}"'"
      owner_before="$(cat "'"${lock3}"'/pid" 2>/dev/null || true)"
      if [[ "${owner_before}" != "$$" ]]; then
        printf "PID_MISMATCH_BEFORE %s got=%s\n" "$$" "${owner_before}" >> "'"${err_log}"'"
      fi
      sleep 0.05
      owner_after="$(cat "'"${lock3}"'/pid" 2>/dev/null || true)"
      if [[ "${owner_after}" != "$$" ]]; then
        printf "PID_MISMATCH_AFTER %s got=%s\n" "$$" "${owner_after}" >> "'"${err_log}"'"
      fi
      printf "EXIT %s\n" "$$" >> "'"${events_log}"'"
      release_lock "'"${lock3}"'"
    ' &
    pids+=($!)
  done

  worker_rc=0
  for p in "${pids[@]}"; do
    wait "${p}" || worker_rc=1
  done
  [[ "${worker_rc}" -eq 0 ]]
  [[ ! -s "${err_log}" ]]
  [[ ! -d "${lock3}" && ! -d "${lock3}.reclaim" ]]

  # Verify all 4 workers completed and max concurrency was strictly 1.
  awk '
    $1 == "ENTER" { cur++; enters++; if (cur > max) max = cur }
    $1 == "EXIT"  { cur--; exits++ }
    END { exit (enters == 4 && exits == 4 && cur == 0 && max == 1 ? 0 : 1) }
  ' "${events_log}"
}
