#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "rust-docs: rust workspace gets all 7 hover settings" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace "$ws"
  [ "$status" -eq 0 ]

  for k in "rust-analyzer.hover.actions.enable" \
           "rust-analyzer.hover.documentation.enable" \
           "rust-analyzer.hover.links.enable" \
           "rust-analyzer.signatureInfo.documentation.enable" \
           "editor.hover.enabled" \
           "editor.hover.delay" \
           "editor.parameterHints.enabled"; do
    val=$(jq --arg k "$k" 'has($k)' "$ws/.vscode/settings.json")
    [ "$val" = "true" ]
  done
}

@test "rust-docs: hover.delay value matches snapshot" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  vspcli --detect --target=workspace "$ws" >/dev/null 2>&1
  delay=$(jq '."editor.hover.delay"' "$ws/.vscode/settings.json")
  [ "$delay" -eq 300 ]
}

@test "rust-docs: tasks.json has both rtk-prefixed cargo doc tasks" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  vspcli --detect --target=workspace "$ws" >/dev/null 2>&1
  [ -f "$ws/.vscode/tasks.json" ]
  doc_cmd=$(jq -r '.tasks[] | select(.label == "rust: doc") | .command' "$ws/.vscode/tasks.json")
  [[ "$doc_cmd" == "rtk cargo doc"* ]]
  strict_cmd=$(jq -r '.tasks[] | select(.label == "rust: doc strict") | .command' "$ws/.vscode/tasks.json")
  [[ "$strict_cmd" == "rtk env RUSTDOCFLAGS"* ]]
}

@test "rust-docs: non-rust workspace omits rust-analyzer keys" {
  require_bin jq
  ws=$(make_workspace astro package.json astro.config.mjs)
  vspcli --detect --target=workspace "$ws" >/dev/null 2>&1
  count=$(jq '[keys[] | select(startswith("rust-analyzer"))] | length' "$ws/.vscode/settings.json")
  [ "$count" -eq 0 ]
}

@test "rust-docs: snapshot file matches what install-workspace emits" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  vspcli --detect --target=workspace "$ws" >/dev/null 2>&1

  snap="$REPO_ROOT/openspec/changes/workspace-toolchain-and-ux-layering/snapshots/rust-tasks.json"
  expected_doc=$(jq -r '.tasks[0].command' "$snap")
  actual_doc=$(jq -r '.tasks[] | select(.label == "rust: doc") | .command' "$ws/.vscode/tasks.json")
  [ "$expected_doc" = "$actual_doc" ]
}
