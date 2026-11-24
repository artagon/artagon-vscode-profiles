#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

# Install a mock curl returning controlled VS Code Marketplace JSON responses.
install_marketplace_curl_mock() {
  local bin_dir="${1}"
  local resp_file="${2}"
  mkdir -p "${bin_dir}"
  cat > "${bin_dir}/curl" <<MOCK
#!/bin/sh
out=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    -o) out="\$2"; shift 2 ;;
    *)  shift ;;
  esac
done
if [ -n "\${out}" ]; then
  cp '${resp_file}' "\${out}"
else
  cat '${resp_file}'
fi
MOCK
  chmod 0755 "${bin_dir}/curl"
  export PATH="${bin_dir}:${PATH}"
}

@test "compat: --check-compat=off skips checker completely (no live curl, no stderr)" {
  ws="$(make_workspace rust Cargo.toml)"
  run --separate-stderr vspcli apply --target=workspace --check-compat=off "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" != *"compat-check:"* ]]
  [[ -f "${ws}/.vscode/extensions.json" ]]
}

@test "compat: invalid --check-compat value exits 4" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --check-compat=zzz "${ws}"
  [[ "${status}" -eq 4 ]]
}

@test "compat: stable-first selection, prerelease-only fallback, incompatible engine exit 1, and NOT_FOUND" {
  install_code_mock "${TEST_TMP}/bin"
  resp="${TEST_TMP}/mp-resp.json"
  cat > "${resp}" <<'EOF'
{
  "results": [{
    "extensions": [
      {
        "publisher": {"publisherName": "rust-lang"},
        "extensionName": "rust-analyzer",
        "versions": [
          {"version": "0.5.0-pre", "properties": [
            {"key": "Microsoft.VisualStudio.Code.PreRelease", "value": "true"},
            {"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.999.0"}
          ]},
          {"version": "0.4.1", "properties": [
            {"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.90.0"}
          ]}
        ]
      },
      {
        "publisher": {"publisherName": "astral-sh"},
        "extensionName": "ty",
        "versions": [
          {"version": "0.0.1-pre", "properties": [
            {"key": "Microsoft.VisualStudio.Code.PreRelease", "value": "true"},
            {"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.95.0"}
          ]}
        ]
      },
      {
        "publisher": {"publisherName": "future"},
        "extensionName": "too-new",
        "versions": [
          {"version": "2.0.0", "properties": [
            {"key": "Microsoft.VisualStudio.Code.Engine", "value": ">=1.200.0"}
          ]}
        ]
      }
    ]
  }]
}
EOF
  install_marketplace_curl_mock "${TEST_TMP}/mp-bin" "${resp}"

  # Case 1: stable-first (rust-analyzer picks 0.4.1 compatible) + prerelease-only fallback (astral-sh.ty) -> exit 0!
  ext_ok="${TEST_TMP}/ok-exts.json"
  printf '[{"identifier":{"id":"rust-lang.rust-analyzer"}},{"identifier":{"id":"astral-sh.ty"}}]\n' > "${ext_ok}"
  run "${BASH}" "${SCRIPTS_DIR}/check-extension-compatibility.sh" --extensions-file "${ext_ok}" --vscode-version 1.147.0 --marketplace-only --json
  [[ "${status}" -eq 0 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.incompatible')" -eq 0 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.not_found')" -eq 0 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.extensions[] | select(.id == "rust-lang.rust-analyzer") | .version')" == "0.4.1" ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.extensions[] | select(.id == "rust-lang.rust-analyzer") | .prerelease')" == "false" ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.extensions[] | select(.id == "astral-sh.ty") | .prerelease')" == "true" ]]

  # Case 2: incompatible engine (>=1.200.0 vs 1.147.0) + missing NOT_FOUND -> exit 1!
  ext_bad="${TEST_TMP}/bad-exts.json"
  printf '[{"identifier":{"id":"future.too-new"}},{"identifier":{"id":"ghost.missing-ext"}}]\n' > "${ext_bad}"
  run "${BASH}" "${SCRIPTS_DIR}/check-extension-compatibility.sh" --extensions-file "${ext_bad}" --vscode-version 1.147.0 --marketplace-only --json
  [[ "${status}" -eq 1 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.incompatible')" -eq 1 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.not_found')" -eq 1 ]]
}

@test "compat: --check-compat=block fails closed BEFORE creating .vscode/ or writing XDG/VS Code files" {
  install_code_mock "${TEST_TMP}/bin"
  # Mock returns empty results so workspace extensions report NOT_FOUND (exit 1).
  resp="${TEST_TMP}/empty-mp.json"
  printf '{"results":[{"extensions":[]}]}\n' > "${resp}"
  install_marketplace_curl_mock "${TEST_TMP}/mp-bin" "${resp}"

  ws="$(make_workspace rust-block Cargo.toml)"
  run vspcli apply --target=workspace --check-compat=block --rtk-in-workspace --ux=crisp --ux-target=user "${ws}"
  [[ "${status}" -eq 1 ]]
  # Fail-closed before ANY mutation: no .vscode/, no XDG RTK install, no User settings.json!
  [[ ! -d "${ws}/.vscode" ]]
  [[ ! -d "${XDG_CONFIG_HOME}/artagon-vscode" ]]
  [[ ! -f "${VSCODE_USER_DIR}/settings.json" ]]
}

@test "compat: --check-compat=block rejects missing checker script cleanly before writing .vscode/" {
  ws="$(make_workspace rust-miss Cargo.toml)"
  # Copy repo scripts WITHOUT check-extension-compatibility.sh to verify fail-closed when checker is absent.
  fake_repo="${TEST_TMP}/fake-repo"
  mkdir -p "${fake_repo}/scripts"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" "${fake_repo}/"
  cp -R "${REPO_ROOT}/scripts/lib" "${fake_repo}/scripts/"
  cp "${REPO_ROOT}/scripts/install-workspace.sh" "${REPO_ROOT}/scripts/detect-toolchain.sh" "${fake_repo}/scripts/"

  run env VSPCLI_ROOT="${fake_repo}" "${BASH}" "${fake_repo}/scripts/install-workspace.sh" --target=workspace --check-compat=block "${ws}"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"checker script missing"* ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "compat: handles partial responses, mismatched/duplicate IDs, and fails closed against offline curl canary" {
  install_code_mock "${TEST_TMP}/bin"
  # 1. Offline canary: without a mock curl, calling check-extension-compatibility.sh hits the fail-closed stub (exit 2).
  ext_canary="${TEST_TMP}/canary.json"
  printf '[{"identifier":{"id":"rust-lang.rust-analyzer"}}]\n' > "${ext_canary}"
  run "${BASH}" "${SCRIPTS_DIR}/check-extension-compatibility.sh" --extensions-file "${ext_canary}" --vscode-version 1.147.0 --marketplace-only --json
  [[ "${status}" -eq 2 ]]

  # 2. Partial response + duplicate input IDs + mismatched ID in response:
  # Input asks for rust-lang.rust-analyzer (duplicated twice) and missing.one;
  # Response returns rust-lang.rust-analyzer + unrelated.extra (partial, missing.one absent).
  resp="${TEST_TMP}/partial-mp.json"
  cat > "${resp}" <<'EOF'
{
  "results": [{
    "extensions": [
      {
        "publisher": {"publisherName": "rust-lang"},
        "extensionName": "rust-analyzer",
        "versions": [{"version": "0.3.3130", "properties": [{"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.144.0"}]}]
      },
      {
        "publisher": {"publisherName": "unrelated"},
        "extensionName": "extra",
        "versions": [{"version": "1.0.0", "properties": [{"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.0.0"}]}]
      }
    ]
  }]
}
EOF
  install_marketplace_curl_mock "${TEST_TMP}/mp-partial" "${resp}"
  ext_dup="${TEST_TMP}/dup.json"
  printf '[{"identifier":{"id":"rust-lang.rust-analyzer"}},{"identifier":{"id":"RUST-LANG.RUST-ANALYZER"}},{"identifier":{"id":"missing.one"}}]\n' > "${ext_dup}"

  run "${BASH}" "${SCRIPTS_DIR}/check-extension-compatibility.sh" --extensions-file "${ext_dup}" --vscode-version 1.147.0 --marketplace-only --json
  [[ "${status}" -eq 1 ]]
  # Duplicate rust-lang.rust-analyzer deduplicated -> total=2, compatible=1, not_found=1!
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.total')" -eq 2 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.compatible')" -eq 1 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.summary.not_found')" -eq 1 ]]
}
