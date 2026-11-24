#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "rust-docs: rust workspace uses VS Code 1.147.0 string editor.hover.enabled='on' and [rust] hover delay 300" {
  require_bin jq
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]

  [[ "$(jq -r '."rust-analyzer.hover.actions.enable"' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq -r '."rust-analyzer.hover.documentation.enable"' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq -r '."rust-analyzer.hover.links.enable"' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq -r '."rust-analyzer.signatureInfo.documentation.enable"' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq -r '."editor.hover.enabled"' "${ws}/.vscode/settings.json")" == "on" ]]
  [[ "$(jq -r '."editor.hover.delay"' "${ws}/.vscode/settings.json")" -eq 200 ]]
  [[ "$(jq -r '."[rust]"."editor.hover.delay"' "${ws}/.vscode/settings.json")" -eq 300 ]]
  [[ "$(jq -r '."editor.parameterHints.enabled"' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq -r '."files.watcherExclude"."**/target/**"' "${ws}/.vscode/settings.json")" == "true" ]]
}

@test "rust-docs: default workspace tasks.json uses portable plain cargo doc commands" {
  require_bin jq
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ -f "${ws}/.vscode/tasks.json" ]]

  doc_cmd="$(jq -r '.tasks[] | select(.label == "rust: doc") | .command' "${ws}/.vscode/tasks.json")"
  [[ "${doc_cmd}" == "cargo doc --workspace --all-features --no-deps" ]]
  strict_cmd="$(jq -r '.tasks[] | select(.label == "rust: doc strict") | .command' "${ws}/.vscode/tasks.json")"
  [[ "${strict_cmd}" == 'env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps' ]]
}

@test "rust-docs: --rtk-in-workspace rewrites cargo doc tasks to rtk-prefixed commands" {
  require_bin jq
  ws="$(make_workspace rust-rtk Cargo.toml)"
  run vspcli apply --target=workspace --rtk-in-workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ -f "${ws}/.vscode/tasks.json" ]]

  doc_cmd="$(jq -r '.tasks[] | select(.label == "rust: doc") | .command' "${ws}/.vscode/tasks.json")"
  [[ "${doc_cmd}" == "rtk cargo doc --workspace --all-features --no-deps" ]]
  strict_cmd="$(jq -r '.tasks[] | select(.label == "rust: doc strict") | .command' "${ws}/.vscode/tasks.json")"
  [[ "${strict_cmd}" == 'rtk env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps' ]]
}

@test "rust-docs: non-rust workspace omits rust-analyzer keys and tasks.json" {
  require_bin jq
  ws="$(make_workspace astro package.json astro.config.mjs)"
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  count="$(jq '[keys[] | select(startswith("rust-analyzer"))] | length' "${ws}/.vscode/settings.json")"
  [[ "${count}" -eq 0 ]]
  [[ ! -f "${ws}/.vscode/tasks.json" ]]
}
