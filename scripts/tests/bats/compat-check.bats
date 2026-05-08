#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "compat: --check-compat=off accepted; install proceeds" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --check-compat=off "$ws"
  [ "$status" -eq 0 ]
}

@test "compat: --check-compat=warn is the default and accepted" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --check-compat=warn "$ws"
  [ "$status" -eq 0 ]
}

@test "compat: --check-compat=block accepted (passes when no incompat)" {
  ws=$(make_workspace rust Cargo.toml)
  # Against current VS Code + post-migration rust profile, no incompat exists.
  run vspcli --detect --target=workspace --check-compat=block "$ws"
  [ "$status" -eq 0 ]
}

@test "compat: invalid --check-compat value exits 4" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --check-compat=zzz "$ws"
  [ "$status" -eq 4 ]
}

@test "compat: --check-compat=off skips the checker entirely (no compat-check stderr)" {
  ws=$(make_workspace rust Cargo.toml)
  run --separate-stderr vspcli --detect --target=workspace --check-compat=off "$ws"
  [ "$status" -eq 0 ]
  # stderr may include other notices but not the compat-check banner.
  [[ "$stderr" != *"compat-check:"* ]]
}
