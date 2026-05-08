#!/usr/bin/env bats
# bats file_tags=no-parallel
# (uses ARTAGON_RTK_WRAPPED env var; cannot run in parallel)

bats_require_minimum_version 1.5.0
load test_helper

@test "rtk-init.bash: sourcing without rtk on PATH produces clear stderr" {
  # Run in a clean PATH that excludes rtk.
  unset ARTAGON_RTK_WRAPPED
  run --separate-stderr env -i HOME="$HOME" PATH="/usr/bin:/bin" \
    bash -c "source $REPO_ROOT/_shared/rtk/rtk-init.bash; echo done"
  [ "$status" -eq 0 ]
  [[ "$stderr" == *"rtk not on PATH"* ]]
}

@test "rtk-init.bash: re-wrap guard suppresses re-init" {
  # Sourcing twice in same shell should be idempotent (returns early on second source).
  result=$(bash -c "
    source $REPO_ROOT/_shared/rtk/rtk-init.bash 2>/dev/null
    source $REPO_ROOT/_shared/rtk/rtk-init.bash 2>/dev/null
    echo \${ARTAGON_RTK_WRAPPED:-unset}
  ")
  [ "$result" = "1" ]
}

@test "rtk-init.bash: only wraps commands present on PATH" {
  # Use a fixture PATH that has rtk + a couple commands, missing others.
  bin=$(mktemp -d)
  printf '#!/bin/sh\necho rtk-stub "$@"\n' > "$bin/rtk"
  chmod +x "$bin/rtk"
  printf '#!/bin/sh\necho real-grep\n' > "$bin/grep"
  chmod +x "$bin/grep"
  result=$(bash -c "
    PATH='$bin:/usr/bin:/bin'
    source $REPO_ROOT/_shared/rtk/rtk-init.bash 2>/dev/null
    type grep | head -1
    type cargo 2>&1 | head -1
  ")
  [[ "$result" == *"is a function"* ]]
  # cargo not on PATH → should NOT be a function (no wrapper installed)
  [[ "$result" != *"cargo is a function"* ]]
  rm -rf "$bin"
}

@test "rtk-init.bash: exported marker is NOT exported (process-local)" {
  bin=$(mktemp -d)
  printf '#!/bin/sh\necho rtk-stub\n' > "$bin/rtk"
  chmod +x "$bin/rtk"
  # Source in a parent shell, fork a child, and check that the child
  # doesn't see ARTAGON_RTK_WRAPPED in its environment.
  result=$(bash -c "
    PATH='$bin:/usr/bin:/bin'
    source $REPO_ROOT/_shared/rtk/rtk-init.bash 2>/dev/null
    bash -c 'echo \${ARTAGON_RTK_WRAPPED:-unset}'
  ")
  [ "$result" = "unset" ]
  rm -rf "$bin"
}

@test "rtk-init.fish: sourcing without rtk produces clear stderr" {
  if ! command -v fish >/dev/null 2>&1; then
    skip "fish not on PATH"
  fi
  fish_path=$(command -v fish)
  fish_dir=$(dirname "$fish_path")
  # Use a sandbox bin dir to host fish-symlink without rtk, so fish can
  # launch but rtk is absent from PATH.
  sandbox=$(mktemp -d)
  ln -s "$fish_path" "$sandbox/fish"
  # Use --no-config to skip user fish config (which may auto-add
  # /opt/homebrew/bin via fish_user_paths and re-pull rtk).
  run --separate-stderr env -i HOME="$HOME" PATH="$sandbox:/usr/bin:/bin" \
    fish --no-config -c "source $REPO_ROOT/_shared/rtk/rtk-init.fish; echo done"
  [[ "$stderr" == *"rtk not on PATH"* ]]
  rm -rf "$sandbox"
  unset fish_dir
}

@test "rtk-init.fish: defines find wrapper when rtk + find on PATH" {
  if ! command -v fish >/dev/null 2>&1; then
    skip "fish not on PATH"
  fi
  bin=$(mktemp -d)
  printf '#!/bin/sh\necho rtk-stub\n' > "$bin/rtk"
  chmod +x "$bin/rtk"
  # Fish only wraps commands actually on PATH. `find` is at /usr/bin/find
  # which we keep available; rg/grep depend on user install. Use find as
  # the canonical test target.
  result=$(fish -c "
    set -gx PATH '$bin' /usr/bin /bin
    source $REPO_ROOT/_shared/rtk/rtk-init.fish >/dev/null 2>&1
    if functions -q find
      echo 'yes'
    else
      echo 'no'
    end
  ")
  [ "$result" = "yes" ]
  rm -rf "$bin"
}
