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

log "All script tests passed."
