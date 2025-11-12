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
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      profile="$2"
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
echo "$(date '+%H:%M:%S') profile=${profile:-unknown}" >> "${CODE_LOG:-/tmp/vscode-tests.log}"
exit 0
EOF
chmod +x "$MOCK_BIN/code"
export PATH="$MOCK_BIN:$PATH"
export CODE_LOG

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
  grep -q "profile=${profile}$" "$CODE_LOG" || fail "Missing log entry for profile $profile"
done < <(profiles)

log "All script tests passed."
