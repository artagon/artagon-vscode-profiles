#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "workspace: detect+install writes .vscode/extensions.json with recommendations" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace "$ws"
  [ "$status" -eq 0 ]
  [ -f "$ws/.vscode/extensions.json" ]
  count=$(jq '.recommendations | length' "$ws/.vscode/extensions.json")
  [ "$count" -ge 22 ]
}

@test "workspace: existing recommendations preserved (set union)" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  mkdir -p "$ws/.vscode"
  cat > "$ws/.vscode/extensions.json" <<EOF
{
  "recommendations": ["existing.foo"],
  "unwantedRecommendations": ["unwanted.bar"]
}
EOF
  run vspcli --detect --target=workspace "$ws"
  [ "$status" -eq 0 ]
  recs=$(jq -r '.recommendations | join(",")' "$ws/.vscode/extensions.json")
  [[ "$recs" == "existing.foo,"* ]]
  [[ "$recs" == *"rust-lang.rust-analyzer"* ]]
  unwanted=$(jq -r '.unwantedRecommendations | join(",")' "$ws/.vscode/extensions.json")
  [ "$unwanted" = "unwanted.bar" ]
}

@test "workspace: idempotent rerun produces same recommendations count" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  vspcli --detect --target=workspace "$ws" >/dev/null 2>&1
  count1=$(jq '.recommendations | length' "$ws/.vscode/extensions.json")
  vspcli --detect --target=workspace "$ws" >/dev/null 2>&1
  count2=$(jq '.recommendations | length' "$ws/.vscode/extensions.json")
  [ "$count1" -eq "$count2" ]
}

@test "workspace: dry-run emits would-be writes without creating files" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --dry-run "$ws"
  [ "$status" -eq 0 ]
  [[ "$output" == *"DRY-RUN"* ]]
  [ ! -f "$ws/.vscode/extensions.json" ]
}
