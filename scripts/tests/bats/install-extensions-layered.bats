#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "compose: rust includes base+rust extensions" {
  require_bin jq
  run bash "$LIB_DIR/compose-extensions.sh" rust
  [ "$status" -eq 0 ]
  count=$(echo "$output" | jq length)
  [ "$count" -gt 22 ]   # 22 base + at least some rust
}

@test "compose: ai layer is exactly Copilot + Copilot Chat" {
  require_bin jq
  run jq -r '.[].identifier.id' "$REPO_ROOT/_shared/extensions/ai.json"
  [ "$status" -eq 0 ]
  [[ "$output" == *"github.copilot"* ]]
  [[ "$output" == *"github.copilot-chat"* ]]
}

@test "compose: rust+ai stacks both layers" {
  require_bin jq
  run bash "$LIB_DIR/compose-extensions.sh" rust ai
  [ "$status" -eq 0 ]
  ids=$(echo "$output" | jq -r '.[].identifier.id')
  echo "$ids" | grep -q "rust-lang.rust-analyzer"
  echo "$ids" | grep -q "github.copilot"
}

@test "compose: dedup preserves first occurrence" {
  require_bin jq
  # ai.json has copilot; ai-plus.json also has copilot. Compose ai+ai-plus and ensure dedup.
  run bash "$LIB_DIR/compose-extensions.sh" ai ai-plus
  [ "$status" -eq 0 ]
  copilot_count=$(echo "$output" | jq '[.[] | select(.identifier.id == "github.copilot")] | length')
  [ "$copilot_count" -eq 1 ]
}

@test "compose: AI extensions absent from rust toolchain layer" {
  require_bin jq
  run jq -r '.[].identifier.id' "$REPO_ROOT/_shared/extensions/rust.json"
  [ "$status" -eq 0 ]
  ! echo "$output" | grep -q "github.copilot"
  ! echo "$output" | grep -q "anthropic.claude"
  ! echo "$output" | grep -q "continue.continue"
}

@test "compose: missing layer file exits non-zero" {
  run bash "$LIB_DIR/compose-extensions.sh" nonexistent-flavor
  [ "$status" -ne 0 ]
  [[ "$output" == *"missing layer file"* ]]
}

@test "compose: post-migration profile extensions.json equals composed output (set-equiv)" {
  require_bin jq
  for flavor in rust astro java-maven java-gradle java-spring cpp-clangd cpp-intellisense ai ai-plus github-workflows; do
    profile_set=$(jq -S '[.[].identifier.id] | sort | unique' "$REPO_ROOT/profiles/$flavor/extensions.json")
    composed_set=$(bash "$LIB_DIR/compose-extensions.sh" "$flavor" | jq -S '[.[].identifier.id] | sort | unique')
    [ "$profile_set" = "$composed_set" ] || {
      echo "set mismatch for flavor $flavor:" >&2
      diff <(echo "$profile_set") <(echo "$composed_set") >&2
      false
    }
  done
}
