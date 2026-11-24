#!/usr/bin/env bats
# bats file_tags=no-parallel

bats_require_minimum_version 1.5.0
load test_helper

setup() {
  setup_hermetic_env
  install_code_mock "${TEST_TMP}/bin"
}

profile_settings() {
  local prof_dir
  prof_dir="$(find "${VSCODE_USER_DIR}/profiles" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  [[ -n "${prof_dir}" ]] || return 1
  printf '%s\n' "${prof_dir}/settings.json"
}

# ---------------------------------------------------------------------------
# Graceful theme fallback (all 4 cases + preferredDarkColorTheme)
# ---------------------------------------------------------------------------

@test "graceful-defaults: strips colorTheme AND preferredDarkColorTheme when theme extension fails" {
  bundle="${TEST_TMP}/with-tokyo.code-profile"
  make_bundle "${bundle}" \
    "workbench.colorTheme=Tokyo Night" \
    "workbench.preferredDarkColorTheme=Tokyo Night" \
    -- enkia.tokyo-night github.copilot-chat
  FAILING_EXTS="enkia.tokyo-night" run import_profile test-profile "${bundle}"
  [[ "${status}" -eq 1 ]]
  settings="$(profile_settings)"
  [[ "$(jq 'has("workbench.colorTheme")' "${settings}")" == "false" ]]
  [[ "$(jq 'has("workbench.preferredDarkColorTheme")' "${settings}")" == "false" ]]
}

@test "graceful-defaults: strips iconTheme and productIconTheme independently when respective extensions fail" {
  bundle="${TEST_TMP}/with-both.code-profile"
  make_bundle "${bundle}" \
    "workbench.colorTheme=Tokyo Night" \
    "workbench.iconTheme=catppuccin-mocha" \
    "workbench.productIconTheme=fluent-icons" \
    -- enkia.tokyo-night catppuccin.catppuccin-vsc-icons miguelsolorio.fluent-icons
  FAILING_EXTS="catppuccin.catppuccin-vsc-icons,miguelsolorio.fluent-icons" run import_profile test-profile "${bundle}"
  [[ "${status}" -eq 1 ]]
  settings="$(profile_settings)"
  [[ "$(jq 'has("workbench.iconTheme")' "${settings}")" == "false" ]]
  [[ "$(jq 'has("workbench.productIconTheme")' "${settings}")" == "false" ]]
  [[ "$(jq -r '."workbench.colorTheme"' "${settings}")" == "Tokyo Night" ]]
}

@test "graceful-defaults: preserves built-in theme name when an unrelated extension fails" {
  bundle="${TEST_TMP}/with-builtin.code-profile"
  make_bundle "${bundle}" \
    "workbench.colorTheme=Default Dark Modern" \
    -- github.copilot-chat enkia.tokyo-night
  FAILING_EXTS="github.copilot-chat" run import_profile test-profile "${bundle}"
  [[ "${status}" -eq 1 ]]
  settings="$(profile_settings)"
  [[ "$(jq -r '."workbench.colorTheme"' "${settings}")" == "Default Dark Modern" ]]
}

@test "graceful-defaults: byte-identical settings on happy path; explicit ID stripped on failure" {
  bundle="${TEST_TMP}/happy.code-profile"
  make_bundle "${bundle}" "workbench.colorTheme=Tokyo Night" -- enkia.tokyo-night
  run import_profile happy-prof "${bundle}"
  [[ "${status}" -eq 0 ]]
  settings="$(profile_settings)"
  diff -u <(jq -S '.settings' "${bundle}") <(jq -S . "${settings}")

  rm -rf "${VSCODE_USER_DIR}/profiles" "${VSCODE_USER_DIR}/globalStorage/storage.json"
  bundle2="${TEST_TMP}/explicit-id.code-profile"
  make_bundle "${bundle2}" "workbench.colorTheme=enkia.tokyo-night" -- enkia.tokyo-night
  FAILING_EXTS="enkia.tokyo-night" run import_profile explicit-prof "${bundle2}"
  [[ "${status}" -eq 1 ]]
  settings2="$(profile_settings)"
  [[ "$(jq 'has("workbench.colorTheme")' "${settings2}")" == "false" ]]
}

# ---------------------------------------------------------------------------
# Pre-v2 bundle migration, settings-only, malformed types, --no-ai, autonomy deny-list
# ---------------------------------------------------------------------------

@test "import: real pre-v2 general.code-profile fixture skips superseded/retired/removed cleanly" {
  fixture="${REPO_ROOT}/scripts/tests/bats/fixtures/pre-v2-general.code-profile"
  [[ -f "${fixture}" ]]
  run --separate-stderr import_profile legacy-gen "${fixture}"
  [[ "${status}" -eq 0 ]]
  # Retired/superseded IDs (foxundermoon.shell-format, mikestead.dotenv, mutantdino.resourcemonitor, oderwat.indent-rainbow) skipped!
  ! grep -q 'ext=foxundermoon.shell-format' "${CODE_LOG}"
  ! grep -q 'ext=mikestead.dotenv' "${CODE_LOG}"
  ! grep -q 'ext=mutantdino.resourcemonitor' "${CODE_LOG}"
  ! grep -q 'ext=oderwat.indent-rainbow' "${CODE_LOG}"
  # Active IDs still installed!
  grep -q 'ext=enkia.tokyo-night' "${CODE_LOG}"
  grep -q 'ext=tamasfe.even-better-toml' "${CODE_LOG}"
}

@test "import: skips compromised extension with security warning and never installs it" {
  bundle="${TEST_TMP}/comp.code-profile"
  make_bundle "${bundle}" -- JScearcy.rust-doc-viewer enkia.tokyo-night
  run --separate-stderr import_profile comp-prof "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"SECURITY"* && "${stderr}" == *"compromised"* ]]
  ! grep -qi 'jscearcy.rust-doc-viewer' "${CODE_LOG}"
  grep -q 'ext=enkia.tokyo-night' "${CODE_LOG}"
}

@test "import: supports settings-only bundle and rejects malformed .settings / .extensions.enabled types" {
  s_only="${TEST_TMP}/settings-only.code-profile"
  printf '{"settings":{"editor.tabSize":4}}\n' > "${s_only}"
  run import_profile settings-only-prof "${s_only}"
  [[ "${status}" -eq 0 ]]

  bad_s="${TEST_TMP}/bad-settings.code-profile"
  printf '{"settings":"oops"}\n' > "${bad_s}"
  run import_profile bad-s-prof "${bad_s}"
  [[ "${status}" -eq 1 ]]

  bad_e="${TEST_TMP}/bad-ext.code-profile"
  printf '{"settings":{},"extensions":{"enabled":"not-an-array"}}\n' > "${bad_e}"
  run import_profile bad-e-prof "${bad_e}"
  [[ "${status}" -eq 1 ]]
}

@test "import: --no-ai strips AI extensions (including Antigravity), chat/inlineChat keys, restores quickSuggestions, and sets chat.disableAIFeatures=true" {
  bundle="${TEST_TMP}/ai-bundle.code-profile"
  jq -n '{
    settings: {
      "editor.tabSize": 2,
      "editor.inlineSuggest.enabled": true,
      "editor.inlineSuggest.fontFamily": "Monaspace Krypton NF",
      "editor.quickSuggestions": { "other": "offWhenInlineCompletions", "comments": "off", "strings": "off" },
      "chat.agent.enabled": true,
      "chat.mcp.customUnenumerated": true,
      "inlineChat.holdToSpeech": false,
      "antigravity.enableInlineDiff": true,
      "claudeCode.preferredLocation": "sidebar"
    },
    extensions: {
      enabled: ["enkia.tokyo-night", "google.google-antigravity", "anthropic.claude-code", "openai.chatgpt", "github.copilot-chat"]
    }
  }' > "${bundle}"
  run import_profile --no-ai no-ai-prof "${bundle}"
  [[ "${status}" -eq 0 ]]
  ! grep -qE 'ext=(google\.google-antigravity|anthropic\.claude-code|openai\.chatgpt|github\.copilot-chat)$' "${CODE_LOG}"
  grep -q 'ext=enkia.tokyo-night' "${CODE_LOG}"
  settings="$(profile_settings)"
  [[ "$(jq -r '."chat.disableAIFeatures"' "${settings}")" == "true" ]]
  [[ "$(jq -r '."editor.quickSuggestions".other' "${settings}")" == "on" ]]
  [[ "$(jq -r '."editor.inlineSuggest.enabled"' "${settings}")" == "true" ]]
  [[ "$(jq 'has("chat.agent.enabled") or has("chat.mcp.customUnenumerated") or has("inlineChat.holdToSpeech") or has("editor.inlineSuggest.fontFamily") or has("antigravity.enableInlineDiff") or has("claudeCode.preferredLocation")' "${settings}")" == "false" ]]
}

@test "import: rejects unsafe autonomy keys in bundle .settings unless --allow-unsafe-settings is passed" {
  bundle="${TEST_TMP}/unsafe.code-profile"
  jq -n '{
    settings: { "chat.tools.terminal.autoApprove": true },
    extensions: { enabled: ["enkia.tokyo-night"] }
  }' > "${bundle}"

  run --separate-stderr import_profile unsafe-prof "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"autonomy deny-list"* ]]
  [[ ! -d "${VSCODE_USER_DIR}/profiles" ]]

  run import_profile --allow-unsafe-settings unsafe-prof "${bundle}"
  [[ "${status}" -eq 0 ]]
}

# ---------------------------------------------------------------------------
# storage.json safety (running VS Code, duplicate names, malformed storage)
# ---------------------------------------------------------------------------

@test "import: refuses storage.json mutation while VS Code is running unless --force" {
  bundle="${TEST_TMP}/run-guard.code-profile"
  make_bundle "${bundle}" -- enkia.tokyo-night

  ARTAGON_VSCODE_TEST_RUNNING=1 run --separate-stderr import_profile guard-prof "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"VS Code appears to be running"* ]]

  ARTAGON_VSCODE_TEST_RUNNING=1 run import_profile --force guard-prof "${bundle}"
  [[ "${status}" -eq 0 ]]
}

@test "import: rejects duplicate profile entries and malformed storage.json cleanly" {
  bundle="${TEST_TMP}/dup.code-profile"
  make_bundle "${bundle}" -- enkia.tokyo-night

  printf '{"userDataProfiles":[{"name":"dup","location":"aaaa1111"},{"name":"dup","location":"bbbb2222"}]}\n' \
    > "${VSCODE_USER_DIR}/globalStorage/storage.json"
  run --separate-stderr import_profile dup "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"duplicate userDataProfiles"* ]]

  printf 'not json\n' > "${VSCODE_USER_DIR}/globalStorage/storage.json"
  run --separate-stderr import_profile broken "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"malformed storage.json"* ]]
}

# ---------------------------------------------------------------------------
# SHA256SUMS, publisher allowlist, and case-insensitive VSIX pins
# ---------------------------------------------------------------------------

@test "sha256sums: match passes silently, mismatch fails closed before write, missing/unlisted warns" {
  bundle_dir="${TEST_TMP}/exports"
  mkdir -p "${bundle_dir}"
  bundle="${bundle_dir}/rust.code-profile"
  make_bundle "${bundle}" -- enkia.tokyo-night

  # Missing manifest -> warn & proceed
  run --separate-stderr import_profile p1 "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"SHA256SUMS not found"* ]]

  # Mismatch -> hard fail before any write
  rm -rf "${VSCODE_USER_DIR}/profiles" "${VSCODE_USER_DIR}/globalStorage/storage.json"
  printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "rust.code-profile" > "${bundle_dir}/SHA256SUMS"
  run --separate-stderr import_profile p2 "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"SHA256 mismatch"* ]]
  [[ ! -d "${VSCODE_USER_DIR}/profiles" ]]

  # Unlisted -> warn & proceed
  printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "other.code-profile" > "${bundle_dir}/SHA256SUMS"
  run --separate-stderr import_profile p3 "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"not listed in SHA256SUMS"* ]]

  # Exact match -> silent success
  source "${LIB_DIR}/common.sh"
  h="$(sha256_for_file "${bundle}")"
  printf '%s  %s\n' "${h}" "rust.code-profile" > "${bundle_dir}/SHA256SUMS"
  run --separate-stderr import_profile p4 "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" != *"SHA256SUMS not found"* && "${stderr}" != *"SHA256 mismatch"* ]]
}

@test "publisher-allowlist: rejects unknown publisher before storage write; env override bypasses with warning" {
  bundle="${TEST_TMP}/typosquat.code-profile"
  make_bundle "${bundle}" -- evilco.bad-actor

  run --separate-stderr import_profile p-evil "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"publisher evilco not in allowlist"* ]]
  [[ ! -d "${VSCODE_USER_DIR}/profiles" ]]

  ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 run --separate-stderr import_profile p-ok "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"unknown publisher evilco"* ]]
}

@test "vsix-pin: case-insensitive lookup verifies SHA-256 VSIX and fails closed on hash mismatch" {
  vsix_payload="${TEST_TMP}/fake-tokyo.vsix"
  printf 'FAKE-VSIX-BINARY-CONTENT-V1\n' > "${vsix_payload}"
  source "${LIB_DIR}/common.sh"
  good_sha="$(sha256_for_file "${vsix_payload}")"

  # Local curl mock serving the fixture VSIX for test URL.
  pin_bin="${TEST_TMP}/pin-bin"
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

  # Uppercase extension ID in bundle matches lowercase pin key!
  bundle="${TEST_TMP}/pinned.code-profile"
  make_bundle "${bundle}" -- ENKIA.Tokyo-Night
  pins="${TEST_TMP}/pins.json"
  jq -n --arg sha "${good_sha}" \
    '{version:1, pins:{"enkia.tokyo-night":{version:"1.1.2", sha256:$sha, vsix_url:"https://marketplace.visualstudio.com/_apis/public/gallery/publishers/enkia/vsextensions/tokyo-night/1.1.2/vspackage"}}}' \
    > "${pins}"

  PATH="${pin_bin}:${PATH}" ARTAGON_VSCODE_PINS_FILE="${pins}" ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1 \
    run import_profile pin-ok "${bundle}"
  [[ "${status}" -eq 0 ]]
  grep -q 'target=.*\.vsix$' "${CODE_LOG}"

  # Hash mismatch must fail closed with zero fallback to live marketplace!
  bad_pins="${TEST_TMP}/bad-pins.json"
  jq -n \
    '{version:1, pins:{"enkia.tokyo-night":{version:"1.1.2", sha256:"0000000000000000000000000000000000000000000000000000000000000000", vsix_url:"https://marketplace.visualstudio.com/_apis/public/gallery/publishers/enkia/vsextensions/tokyo-night/1.1.2/vspackage"}}}' \
    > "${bad_pins}"
  : > "${CODE_LOG}"
  PATH="${pin_bin}:${PATH}" ARTAGON_VSCODE_PINS_FILE="${bad_pins}" ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1 \
    run --separate-stderr import_profile pin-bad "${bundle}"
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"sha256 mismatch"* ]]
  ! grep -q 'action=install' "${CODE_LOG}"

  # ARTAGON_VSCODE_BYPASS_PINS=1 skips pin fetch with warning.
  ARTAGON_VSCODE_PINS_FILE="${bad_pins}" ARTAGON_VSCODE_BYPASS_PINS=1 \
    run --separate-stderr import_profile pin-bypass "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"bypassing pin manifest"* ]]
}

@test "storage: skips occupied on-disk profiles/<id> dir and aborts on concurrent CAS mutation" {
  # 1. Occupied-ID reservation: stub rand_hex_8 to return an already-occupied dir ID first, then a free ID.
  mkdir -p "${VSCODE_USER_DIR}/profiles/deadbeef"
  calls_file="${TEST_TMP}/rand-hex-calls"
  printf '0\n' > "${calls_file}"
  run "${BASH}" -c "
    source '${LIB_DIR}/common.sh'
    source '${LIB_DIR}/vscode-storage.sh'
    rand_hex_8() {
      local c
      c=\"\$(cat '${calls_file}')\"
      c=\$((c + 1))
      printf '%d\n' \"\${c}\" > '${calls_file}'
      if [[ \"\${c}\" -eq 1 ]]; then
        printf 'deadbeef\n'
      else
        printf 'cafebabe\n'
      fi
    }
    vscode_storage_ensure_profile '${VSCODE_USER_DIR}' 'occupied-test'
  "
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == "cafebabe" ]]
  [[ -d "${VSCODE_USER_DIR}/profiles/cafebabe" ]]

  # 2. CAS concurrent-writer guard: simulate an independent writer mutating storage.json right before publish!
  printf '{"userDataProfiles":[]}\n' > "${VSCODE_USER_DIR}/globalStorage/storage.json"
  run --separate-stderr "${BASH}" -c "
    source '${LIB_DIR}/common.sh'
    source '${LIB_DIR}/vscode-storage.sh'
    backup_user_file() {
      printf '{\"userDataProfiles\":[{\"name\":\"racer\",\"location\":\"11112222\"}]}\n' > '${VSCODE_USER_DIR}/globalStorage/storage.json'
      return 0
    }
    vscode_storage_ensure_profile '${VSCODE_USER_DIR}' 'cas-victim'
  "
  [[ "${status}" -eq 1 ]]
  [[ "${stderr}" == *"changed concurrently"* ]]
  # Verify racer's entry was NOT clobbered!
  [[ "$(jq -r '.userDataProfiles[0].name' "${VSCODE_USER_DIR}/globalStorage/storage.json")" == "racer" ]]
}

@test "vsix-pin: verified VSIX cache serves SHA-256 hit offline and rejects tampered cache file" {
  vsix_payload="${TEST_TMP}/cached-tokyo.vsix"
  printf 'VERIFIED-CACHED-VSIX-BYTES\n' > "${vsix_payload}"
  source "${LIB_DIR}/common.sh"
  good_sha="$(sha256_for_file "${vsix_payload}")"
  url="https://marketplace.visualstudio.com/_apis/public/gallery/publishers/enkia/vsextensions/tokyo-night/1.1.2/vspackage"

  # 1. Valid cache entry serves hit WITHOUT calling curl (offline curl stub exits 99 if called).
  cache_file="${VSCODE_VSIX_CACHE_DIR}/enkia.tokyo-night-${good_sha}.vsix"
  cp "${vsix_payload}" "${cache_file}"
  out_ok="${TEST_TMP}/out-ok.vsix"
  run "${BASH}" -c "source '${LIB_DIR}/vsix-pin.sh'; fetch_and_verify_vsix 'ENKIA.Tokyo-Night' '${url}' '${good_sha}' '${out_ok}'"
  [[ "${status}" -eq 0 ]]
  [[ "$(sha256_for_file "${out_ok}")" == "${good_sha}" ]]

  # 2. Tampered cache file is evicted and fails closed when offline curl cannot re-fetch.
  printf 'TAMPERED-BYTES\n' > "${cache_file}"
  out_bad="${TEST_TMP}/out-bad.vsix"
  run --separate-stderr "${BASH}" -c "source '${LIB_DIR}/vsix-pin.sh'; fetch_and_verify_vsix 'enkia.tokyo-night' '${url}' '${good_sha}' '${out_bad}'"
  [[ "${status}" -ne 0 ]]
  [[ ! -f "${cache_file}" ]]
  [[ ! -f "${out_bad}" ]]

  # 3. Cache miss populates VSCODE_VSIX_CACHE_DIR atomically (0600) on macOS BSD chmod & Linux GNU chmod.
  pin_bin="${TEST_TMP}/pin-cache-bin"
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
  out_miss="${TEST_TMP}/out-miss.vsix"
  PATH="${pin_bin}:${PATH}" ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1 \
    run "${BASH}" -c "source '${LIB_DIR}/vsix-pin.sh'; fetch_and_verify_vsix 'enkia.tokyo-night' '${url}' '${good_sha}' '${out_miss}'"
  [[ "${status}" -eq 0 ]]
  [[ -f "${cache_file}" ]]
  [[ "$(sha256_for_file "${cache_file}")" == "${good_sha}" ]]
}

@test "import: batched install runs in one code invocation and falls back cleanly on single-extension failure" {
  bundle="${TEST_TMP}/batch.code-profile"
  make_bundle "${bundle}" "workbench.colorTheme=Tokyo Night" -- enkia.tokyo-night github.copilot-chat editorconfig.editorconfig

  # Happy path: all 3 extensions installed in a SINGLE batched `code` invocation!
  run import_profile batch-ok "${bundle}"
  [[ "${status}" -eq 0 ]]
  [[ "$(grep -c 'action=install_call count=3 ' "${CODE_LOG}")" -eq 1 ]]

  # Single-extension failure in batch falls back cleanly to per-extension attribution & strips dangling theme.
  rm -rf "${VSCODE_USER_DIR}/profiles" "${VSCODE_USER_DIR}/globalStorage/storage.json"
  : > "${CODE_LOG}"
  FAILING_EXTS="enkia.tokyo-night" run import_profile batch-fallback "${bundle}"
  [[ "${status}" -eq 1 ]]
  settings="$(profile_settings)"
  [[ "$(jq 'has("workbench.colorTheme")' "${settings}")" == "false" ]]
  grep -q 'action=install .*ext=github.copilot-chat ' "${CODE_LOG}"
  grep -q 'action=install .*ext=editorconfig.editorconfig ' "${CODE_LOG}"
}

@test "common: is_vscode_running detects macOS & Linux VS Code binaries while ignoring out/cli.js and unrelated commands" {
  stub_bin="${TEST_TMP}/ps-stub"
  mkdir -p "${stub_bin}"
  cat > "${stub_bin}/uname" <<'EOF'
#!/bin/sh
printf '%s\n' "${FAKE_UNAME:-Darwin}"
EOF
  cat > "${stub_bin}/ps" <<'EOF'
#!/bin/sh
printf '%s\n' "${FAKE_PS_OUT}"
EOF
  chmod 0755 "${stub_bin}/uname" "${stub_bin}/ps"

  # 1. macOS desktop binary Contents/MacOS/Code => detected (exit 0).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" \
    FAKE_PS_OUT="/Applications/Visual Studio Code.app/Contents/MacOS/Code" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 0 ]]

  # 2. macOS transient CLI out/cli.js => NOT detected (exit 1).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" \
    FAKE_PS_OUT="/Applications/Visual Studio Code.app/Contents/MacOS/Code /Applications/Visual Studio Code.app/Contents/Resources/app/out/cli.js --profile ai --install-extension foo.bar" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 1 ]]

  # 3. macOS unrelated command mentioning VS Code path in argv[1+] => NOT detected (exit 1).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Darwin" \
    FAKE_PS_OUT="codesign -dv /Applications/Visual Studio Code.app/Contents/MacOS/Code" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 1 ]]

  # 4. Linux desktop binary /usr/share/code/code => detected (exit 0), out/cli.js & ls /home/u/code => NOT detected (exit 1).
  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Linux" \
    FAKE_PS_OUT="/usr/share/code/code --unity-launch" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 0 ]]

  PATH="${stub_bin}:${PATH}" ARTAGON_VSCODE_TEST_RUNNING="" FAKE_UNAME="Linux" \
    FAKE_PS_OUT="ls /home/user/code
/usr/share/code/code /usr/share/code/resources/app/out/cli.js --install-extension foo.bar" \
    run "${BASH}" -c "source '${LIB_DIR}/common.sh'; is_vscode_running"
  [[ "${status}" -eq 1 ]]
}
