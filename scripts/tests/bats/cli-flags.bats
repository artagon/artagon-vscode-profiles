#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "cli: --help returns help text" {
  run vspcli --help
  [ "$status" -eq 0 ]
  [[ "$output" == *"vspcli"* ]]
  [[ "$output" == *"--list"* ]]
}

@test "cli: --list lists 11 post-migration flavors" {
  run vspcli --list
  [ "$status" -eq 0 ]
  count=$(echo "$output" | wc -l | tr -d ' ')
  [ "$count" -eq 11 ]
  [[ "$output" == *"rust"* ]]
  [[ "$output" == *"astro"* ]]
  [[ "$output" == *"general"* ]]
}

@test "cli: --completion bash emits a snippet" {
  run vspcli --completion bash
  [ "$status" -eq 0 ]
  [[ "$output" == *"_vspcli_completions"* ]]
  [[ "$output" == *"complete -F"* ]]
}

@test "cli: --completion zsh emits a snippet" {
  run vspcli --completion zsh
  [ "$status" -eq 0 ]
  [[ "$output" == *"#compdef vspcli"* ]]
  [[ "$output" == *"_arguments"* ]]
}

@test "cli: --completion fish emits a snippet" {
  run vspcli --completion fish
  [ "$status" -eq 0 ]
  [[ "$output" == *"complete -c vspcli"* ]]
}

@test "cli: --completion powershell exits non-zero" {
  run vspcli --completion powershell
  [ "$status" -ne 0 ]
}

@test "cli: --detect routes to detection script" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "rust" ]
}

@test "cli: --migrate-catalog --doctor returns 0 when clean" {
  run vspcli --migrate-catalog --doctor
  [ "$status" -eq 0 ]
}

@test "cli: unknown flag exits non-zero" {
  run vspcli --xx-bogus-flag
  [ "$status" -ne 0 ]
}

@test "cli: --detect --target=workspace --dry-run does not create files" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --dry-run "$ws"
  [ "$status" -eq 0 ]
  [ ! -d "$ws/.vscode" ]
}

@test "cli: --detect --target=invalid exits 4" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=zzz "$ws"
  [ "$status" -eq 4 ]
}
