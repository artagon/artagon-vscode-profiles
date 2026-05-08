#!/usr/bin/env bats
# bats file_tags=:no-parallel

bats_require_minimum_version 1.5.0
load test_helper

@test "legacy: rust-profile-crisp resolves to 'rust crisp'" {
  source "$LIB_DIR/legacy-profile-name.sh"
  result=$(resolve_legacy_profile_name "rust-profile-crisp" 2>/dev/null)
  [ "$result" = "rust crisp" ]
}

@test "legacy: web-astro-retina resolves to 'astro retina'" {
  source "$LIB_DIR/legacy-profile-name.sh"
  result=$(resolve_legacy_profile_name "web-astro-retina" 2>/dev/null)
  [ "$result" = "astro retina" ]
}

@test "legacy: ai-profile-crisp resolves to 'ai crisp'" {
  source "$LIB_DIR/legacy-profile-name.sh"
  result=$(resolve_legacy_profile_name "ai-profile-crisp" 2>/dev/null)
  [ "$result" = "ai crisp" ]
}

@test "legacy: java-profile aliases to java-maven" {
  source "$LIB_DIR/legacy-profile-name.sh"
  result=$(resolve_legacy_profile_name "java-profile-crisp" 2>/dev/null)
  [ "$result" = "java-maven crisp" ]
}

@test "legacy: non-legacy name returns empty" {
  source "$LIB_DIR/legacy-profile-name.sh"
  result=$(resolve_legacy_profile_name "rust" 2>/dev/null)
  [ -z "$result" ]
}

@test "legacy: deprecation warning emitted on first invocation" {
  unset ARTAGON_VSCODE_DEPRECATION_SEEN
  source "$LIB_DIR/legacy-profile-name.sh"
  run --separate-stderr resolve_legacy_profile_name "rust-profile-crisp"
  [[ "$stderr" == *"deprecation"* ]]
  [[ "$stderr" == *"rust-profile-crisp"* ]]
}

@test "legacy: deprecation warning suppressed by ARTAGON_VSCODE_DEPRECATION_ACK" {
  export ARTAGON_VSCODE_DEPRECATION_ACK=1
  source "$LIB_DIR/legacy-profile-name.sh"
  run --separate-stderr resolve_legacy_profile_name "rust-profile-crisp"
  [ -z "$stderr" ]
  [ "$output" = "rust crisp" ]
}

@test "legacy: warning suppressed on second invocation in same shell" {
  unset ARTAGON_VSCODE_DEPRECATION_SEEN
  source "$LIB_DIR/legacy-profile-name.sh"
  resolve_legacy_profile_name "rust-profile-crisp" >/dev/null 2>&1
  # Second call: ARTAGON_VSCODE_DEPRECATION_SEEN is now set; no warning.
  run --separate-stderr resolve_legacy_profile_name "java-spring-retina"
  [ -z "$stderr" ]
  [ "$output" = "java-spring retina" ]
}

@test "migration: --doctor reports clean state when migrated" {
  run bash "$SCRIPTS_DIR/migrate-catalog.sh" --doctor
  [ "$status" -eq 0 ]
  [[ "$output" == *"fully migrated"* ]] || [[ "$output" == *"ok"* ]]
}

@test "migration: post-migration profiles dir has 11 entries" {
  count=$(find "$REPO_ROOT/profiles" -mindepth 1 -maxdepth 1 -type d | wc -l)
  [ "$count" -eq 11 ]
}

@test "migration: legacy bundle filenames are symlinks to canonical" {
  legacy="$REPO_ROOT/exports/rust-profile-crisp.code-profile"
  [ -L "$legacy" ]
  target=$(readlink "$legacy")
  [ "$target" = "rust.code-profile" ]
}
