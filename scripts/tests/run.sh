#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP="$(mktemp -d -t vscode-tests-XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

log() { printf '%s\n' "TEST: $*"; }
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# Mock VS Code CLI so open-profiles.sh can run without launching UI.
MOCK_BIN="$TMP/bin"
CODE_LOG="$TMP/code.log"
mkdir -p "$MOCK_BIN"
cat <<'EOF' > "$MOCK_BIN/code"
#!/usr/bin/env bash
profile=""
action="open"
ext=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      profile="$2"
      shift 2
      ;;
    --install-extension)
      action="install"
      ext="$2"
      shift 2
      ;;
    --new-window)
      shift
      ;;
    *)
      shift
      ;;
  esac
done
timestamp="$(date '+%H:%M:%S')"
if [[ "$action" == "install" ]]; then
  echo "${timestamp} action=install profile=${profile:-unknown} ext=${ext}" >> "${CODE_LOG:-/tmp/vscode-tests.log}"
else
  echo "${timestamp} action=open profile=${profile:-unknown}" >> "${CODE_LOG:-/tmp/vscode-tests.log}"
fi
exit 0
EOF
chmod +x "$MOCK_BIN/code"
export PATH="$MOCK_BIN:$PATH"
export CODE_LOG
export VSCODE_EXTENSION_INSTALL_DELAY=0

profiles() {
  find "$ROOT/profiles" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort
}

log "validate-json.sh"
bash "$ROOT/scripts/validate-json.sh" >/dev/null

log "compose-settings.sh"
bash "$ROOT/scripts/compose-settings.sh" >/dev/null

symlink="$ROOT/profiles/java-profile-crisp/settings.json"
[ -L "$symlink" ] || fail "Expected symlink at $symlink"

log "export-profiles.sh"
bash "$ROOT/scripts/export-profiles.sh" >/dev/null

export_file="$ROOT/exports/java-profile-crisp.code-profile"
[ -s "$export_file" ] || fail "Expected export at $export_file"

log "open-profiles.sh (mocked code CLI + log monitoring)"
rm -f "$CODE_LOG"
export VSCODE_SKIP_EXTENSION_INSTALL=1
bash "$ROOT/scripts/open-profiles.sh" >/dev/null
unset VSCODE_SKIP_EXTENSION_INSTALL

expected_lines="$(profiles | wc -l | tr -d ' ')"
actual_lines="$(wc -l < "$CODE_LOG" 2>/dev/null || echo 0)"
[ "$actual_lines" -eq "$expected_lines" ] || fail "Expected $expected_lines code invocations, saw $actual_lines"

while read -r profile; do
  grep -q "action=open profile=${profile}$" "$CODE_LOG" || fail "Missing log entry for profile $profile"
done < <(profiles)

log "install-extensions.sh honors group filters"
rm -f "$CODE_LOG"
export VSCODE_EXTENSION_INSTALL_DELAY=0
bash "$ROOT/scripts/install-extensions.sh" java-profile-crisp --group AI >/dev/null
ai_count="$(jq -r '.[].identifier.id' "$ROOT/profiles/java-profile-crisp/extensions.json" | grep -iE '^(github\.copilot|github\.copilot-chat|anthropic\.claude-code|openai\.chatgpt|googlecloudtools\.cloudcode|continue\.continue|codeium\.codeium|tabnine\.tabnine-vscode|sourcegraph\.cody-ai|sourcegraph\.amp)$' || true)"
ai_count="$(printf '%s\n' "$ai_count" | sed '/^$/d' | wc -l | tr -d ' ')"
logged_ai="$( { grep -c 'action=install' "$CODE_LOG" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[ "$ai_count" -eq "$logged_ai" ] || fail "Expected $ai_count AI installs, saw $logged_ai"

log "open-profiles.sh skips installs when cache hash matches"
rm -f "$CODE_LOG"
rm -f "$ROOT/.cache/extensions-installed/ai-profile-crisp"
bash "$ROOT/scripts/open-profiles.sh" ai-profile-crisp >/dev/null
first_installs="$( { grep -c 'action=install' "$CODE_LOG" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[ "$first_installs" -gt 0 ] || fail "Expected initial install actions"
rm -f "$CODE_LOG"
bash "$ROOT/scripts/open-profiles.sh" ai-profile-crisp >/dev/null
second_installs="$( { grep -c 'action=install' "$CODE_LOG" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[ "$second_installs" -eq 0 ] || fail "Expected cache to skip installs; saw $second_installs"

log "compose-settings.sh @extends path-containment"
# Build a self-contained mini-repo with the same layout the script expects
# (scripts/ at the top, _shared/_overrides/profiles siblings) and invoke compose
# from inside it so its $(cd "$(dirname "$0")/..") resolves to the fixture root.
COMPOSE_FIXTURE_DIR="$TMP/repo"
mkdir -p "$COMPOSE_FIXTURE_DIR/scripts" "$COMPOSE_FIXTURE_DIR/_shared" \
  "$COMPOSE_FIXTURE_DIR/_overrides" "$COMPOSE_FIXTURE_DIR/profiles/test-fixture"
echo '{}' > "$COMPOSE_FIXTURE_DIR/_shared/editor-crisp.jsonc"
echo '{}' > "$COMPOSE_FIXTURE_DIR/_shared/editor-retina.jsonc"
cp "$ROOT/scripts/compose-settings.sh" "$COMPOSE_FIXTURE_DIR/scripts/compose-settings.sh"
run_compose() {
  bash "$COMPOSE_FIXTURE_DIR/scripts/compose-settings.sh" "$@" 2>&1
}

# Case 1: traversal
cat > "$COMPOSE_FIXTURE_DIR/_overrides/test-fixture.jsonc" <<'EOF'
{ "@extends": ["../../../etc/passwd"] }
EOF
out="$(run_compose test-fixture || true)"
echo "$out" | grep -q "rejected @extends path" || fail "Expected traversal rejection, got: $out"

# Case 2: absolute path
cat > "$COMPOSE_FIXTURE_DIR/_overrides/test-fixture.jsonc" <<'EOF'
{ "@extends": ["/tmp/evil.jsonc"] }
EOF
out="$(run_compose test-fixture || true)"
echo "$out" | grep -q "rejected @extends path" || fail "Expected absolute-path rejection, got: $out"

# Case 3: home-prefixed
cat > "$COMPOSE_FIXTURE_DIR/_overrides/test-fixture.jsonc" <<'EOF'
{ "@extends": ["~/secret.jsonc"] }
EOF
out="$(run_compose test-fixture || true)"
echo "$out" | grep -q "rejected @extends path" || fail "Expected home-prefix rejection, got: $out"

# Case 4: malformed @extends value (non-string, non-array)
cat > "$COMPOSE_FIXTURE_DIR/_overrides/test-fixture.jsonc" <<'EOF'
{ "@extends": 1 }
EOF
out="$(run_compose test-fixture || true)"
echo "$out" | grep -qiE "@extends|jq: error" || fail "Expected malformed-@extends error, got: $out"

# Case 5: cycle through symlink
cat > "$COMPOSE_FIXTURE_DIR/_overrides/cycle-base.jsonc" <<'EOF'
{ "@extends": ["cycle-link.jsonc"] }
EOF
ln -sf cycle-base.jsonc "$COMPOSE_FIXTURE_DIR/_overrides/cycle-link.jsonc"
cat > "$COMPOSE_FIXTURE_DIR/_overrides/test-fixture.jsonc" <<'EOF'
{ "@extends": ["cycle-base.jsonc"] }
EOF
out="$(run_compose test-fixture || true)"
echo "$out" | grep -qi "cycle" || fail "Expected cycle detection through symlink, got: $out"

log "extension-id allowlist rejects injection-shaped values across all three paths"
INJ_ID='evil; rm -rf /tmp/x'
INJ_PROFILE="$TMP/injection-profile"
mkdir -p "$INJ_PROFILE"
cat > "$INJ_PROFILE/extensions.json" <<EOF
[ { "identifier": { "id": "${INJ_ID}" } } ]
EOF
# Path A: install-extensions.sh — point profiles dir at our fixture by symlinking
mkdir -p "$TMP/profiles-fixture"
ln -sfn "$INJ_PROFILE" "$TMP/profiles-fixture/injection-profile"
out_a="$( ROOT_OVERRIDE="$TMP/profiles-fixture" bash -c '
  set +e
  EXT_FILE="$0/injection-profile/extensions.json"
  source "'"$ROOT"'/scripts/lib/extension-id.sh"
  ext="$(jq -r ".[].identifier.id" "$EXT_FILE")"
  validate_extension_id "$ext"
  echo "exit=$?"
' "$TMP/profiles-fixture" 2>&1)"
echo "$out_a" | grep -q 'exit=1' || fail "Expected install-extensions path to reject injection id, got: $out_a"

# Path B: import-profile.sh — feed a poisoned bundle
INJ_BUNDLE="$TMP/injection.code-profile"
cat > "$INJ_BUNDLE" <<EOF
{ "settings": {}, "extensions": { "enabled": ["${INJ_ID}"] } }
EOF
out_b="$( bash -c '
  set +e
  source "'"$ROOT"'/scripts/lib/extension-id.sh"
  ext="$(jq -r ".extensions.enabled[]" "$0")"
  validate_extension_id "$ext"
  echo "exit=$?"
' "$INJ_BUNDLE" 2>&1)"
echo "$out_b" | grep -q 'exit=1' || fail "Expected import-profile path to reject injection id, got: $out_b"

# Path C: vspcli --install-ext direct value
out_c="$( bash -c '
  set +e
  source "'"$ROOT"'/scripts/lib/extension-id.sh"
  validate_extension_id "$0"
  echo "exit=$?"
' "$INJ_ID" 2>&1)"
echo "$out_c" | grep -q 'exit=1' || fail "Expected vspcli path to reject injection id, got: $out_c"

log "import-profile.sh PROFILE_ID generator is safe under set -euo pipefail"
out_d="$( bash -c '
  set -euo pipefail
  if command -v openssl >/dev/null 2>&1; then
    PROFILE_ID="$(openssl rand -hex 4)"
  else
    PROFILE_ID="$(python3 -c "import secrets; print(secrets.token_hex(4))")"
  fi
  echo "id=$PROFILE_ID"
' 2>&1)"
echo "$out_d" | grep -qE 'id=[0-9a-f]{8}$' || fail "Expected 8-char hex PROFILE_ID under set -euo pipefail, got: $out_d"

log "extension-id regex literal in helper matches the spec deltas"
HELPER_REGEX="$(grep -E "^EXTENSION_ID_REGEX=" "$ROOT/scripts/lib/extension-id.sh" | sed -E "s/^EXTENSION_ID_REGEX=//; s/^'//; s/'$//")"
[ -n "$HELPER_REGEX" ] || fail "Could not extract EXTENSION_ID_REGEX literal from helper"
for spec in install-profile-extensions import-profile-bundles manage-profile-cli; do
  spec_path="$ROOT/openspec/changes/harden-profile-tooling-and-pipeline/specs/$spec/spec.md"
  grep -F -- "$HELPER_REGEX" "$spec_path" >/dev/null \
    || fail "Helper regex literal not found verbatim in $spec_path"
done

log "all <stack>-{crisp,retina}.jsonc leaves remain symlinks to <stack>-base.jsonc"
# The composer's filename-keyed base selection (compose-settings.sh:38-39)
# makes the symlink trick load-bearing for ANY leaf <stack>-{crisp,retina}.jsonc
# whose content is shared with <stack>-base.jsonc. Currently 10 such symlinks
# across java-{gradle,maven,profile,spring} and rust-profile.
for stack in java-gradle java-maven java-profile java-spring rust-profile; do
  for variant in crisp retina; do
    link="$ROOT/_overrides/${stack}-${variant}.jsonc"
    [ -L "$link" ] || fail "Expected symlink at $link (was a regular file — pattern broken)"
    target="$(readlink "$link")"
    expected="${stack}-base.jsonc"
    [ "$target" = "$expected" ] || fail "Expected $link -> $expected, got $target"
  done
done

log "every merged profile has security.workspace.trust.untrustedFiles=prompt"
# Contracted by openspec/changes/.../specs/secure-shared-defaults/spec.md.
# A regression in _shared/editor-{crisp,retina}.jsonc would silently flip
# trust back to "open" across all 22 profiles; this guard catches it before
# the CI 'git diff' step.
mismatched=()
for merged in "$ROOT"/_merged/*.json; do
  val="$(jq -r '."security.workspace.trust.untrustedFiles" // "MISSING"' "$merged")"
  [ "$val" = "prompt" ] || mismatched+=("$(basename "$merged"):$val")
done
[ "${#mismatched[@]}" -eq 0 ] || fail "Workspace Trust value not 'prompt' in: ${mismatched[*]}"

log "All script tests passed."
