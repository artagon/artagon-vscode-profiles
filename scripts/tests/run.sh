#!/usr/bin/env bash
# run.sh — Hermetic offline test runner (smoke checks + full BATS suite).
# Isolates HOME, XDG_CONFIG_HOME, XDG_STATE_HOME, VSCODE_USER_DIR,
# VSCODE_EXT_CACHE_DIR, ARTAGON_VSCODE_PINS_FILE, and blocks live curl calls
# BEFORE invoking any script.
set -euo pipefail

ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/../.." && pwd -P)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/vscode-tests-XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT

log()  { printf '%s\n' "TEST: ${*}"; }
fail() { printf 'ERROR: %s\n' "${*}" >&2; exit 1; }

# Preserve real Cargo/Rustup paths before isolating HOME (for rust-docs-strict-gate.bats).
export CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-${HOME}/.rustup}"

export HOME="${TMP}/home"
export XDG_CONFIG_HOME="${TMP}/xdg-config"
export XDG_STATE_HOME="${TMP}/xdg-state"
export VSCODE_USER_DIR="${TMP}/vscode/User"
export VSCODE_EXT_CACHE_DIR="${TMP}/ext-cache"
export VSCODE_VSIX_CACHE_DIR="${TMP}/vsix-cache"
export ARTAGON_VSCODE_TEST_RUNNING=0
export VSCODE_EXTENSION_INSTALL_DELAY=0

mkdir -p "${HOME}" "${XDG_CONFIG_HOME}" "${XDG_STATE_HOME}" \
  "${VSCODE_USER_DIR}/globalStorage" "${VSCODE_USER_DIR}/profiles/sentinel00" "${VSCODE_EXT_CACHE_DIR}" "${VSCODE_VSIX_CACHE_DIR}"
printf '{"sentinel":true}\n' > "${VSCODE_USER_DIR}/profiles/sentinel00/settings.json"
printf 'outer-cache-sentinel\n' > "${VSCODE_EXT_CACHE_DIR}/__sentinel__"

export ARTAGON_VSCODE_PINS_FILE="${TMP}/empty-pins.json"
printf '{"version":1,"pins":{}}\n' > "${ARTAGON_VSCODE_PINS_FILE}"

# Install fail-closed curl stub + mocked VS Code CLI on PATH.
MOCK_BIN="${TMP}/bin"
CODE_LOG="${TMP}/code.log"
mkdir -p "${MOCK_BIN}"

cat > "${MOCK_BIN}/curl" <<'STUB'
#!/bin/sh
echo "ERROR: unexpected live curl call in offline run.sh: $*" >&2
exit 99
STUB
chmod 0755 "${MOCK_BIN}/curl"

cat > "${MOCK_BIN}/code" <<'EOF'
#!/usr/bin/env bash
profile="default"
action="open"
ext=""
install_exts=()
while [[ $# -gt 0 ]]; do
  case "${1}" in
    --version)            printf '1.147.0\n0000000\narm64\n'; exit 0 ;;
    --user-data-dir)      shift 2 ;;
    --profile)            profile="${2}"; shift 2 ;;
    --install-extension)  action="install"; install_exts+=("${2}"); shift 2 ;;
    --uninstall-extension) action="uninstall"; ext="${2}"; shift 2 ;;
    --list-extensions)    action="list"; shift ;;
    --show-versions)      shift ;;
    --new-window)         shift ;;
    *)                    shift ;;
  esac
done
timestamp="$(date '+%H:%M:%S')"
case "${action}" in
  install)
    for one_ext in "${install_exts[@]+"${install_exts[@]}"}"; do
      echo "${timestamp} action=install profile=${profile} ext=${one_ext}" >> "${CODE_LOG}"
    done
    ;;
  uninstall) echo "${timestamp} action=uninstall profile=${profile} ext=${ext}" >> "${CODE_LOG}" ;;
  list)      ;;
  *)         echo "${timestamp} action=open profile=${profile}" >> "${CODE_LOG}" ;;
esac
exit 0
EOF
chmod 0755 "${MOCK_BIN}/code"
export PATH="${MOCK_BIN}:${PATH}"
export CODE_LOG

profiles() {
  find "${ROOT}/profiles" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | LC_ALL=C sort
}

log "validate-json.sh + check-ux-symmetry.sh + rust policy + readme matrix"
"${BASH}" "${ROOT}/scripts/validate-json.sh" >/dev/null
"${BASH}" "${ROOT}/scripts/tests/check-ux-symmetry.sh" >/dev/null
"${BASH}" "${ROOT}/scripts/lib/apply-rust-policy.sh" --check >/dev/null
"${BASH}" "${ROOT}/scripts/generate-readme-matrix.sh" --check >/dev/null

log "vspcli build (compose + export + 22 legacy symlinks + SHA256SUMS)"
"${BASH}" "${ROOT}/scripts/vspcli" build >/dev/null

symlink="${ROOT}/profiles/java-maven/settings.json"
[[ -L "${symlink}" ]] || fail "Expected symlink at ${symlink}"
export_file="${ROOT}/exports/java-maven.code-profile"
[[ -s "${export_file}" ]] || fail "Expected export at ${export_file}"
[[ "$(readlink "${ROOT}/exports/ai-profile-crisp.code-profile")" == "ai-copilot.code-profile" ]] \
  || fail "Expected ai-profile-crisp.code-profile -> ai-copilot.code-profile"
[[ "$(readlink "${ROOT}/exports/ai-profile-retina.code-profile")" == "ai-copilot.code-profile" ]] \
  || fail "Expected ai-profile-retina.code-profile -> ai-copilot.code-profile"

log "open-profiles.sh (mocked code CLI across all 15 recipes)"
rm -f "${CODE_LOG}"
VSCODE_SKIP_EXTENSION_INSTALL=1 "${BASH}" "${ROOT}/scripts/open-profiles.sh" >/dev/null

expected_lines="$(profiles | wc -l | tr -d ' ')"
[[ "${expected_lines}" -eq 15 ]] || fail "Expected 15 profiles, saw ${expected_lines}"
actual_lines="$(wc -l < "${CODE_LOG}" 2>/dev/null | tr -d ' ' || echo 0)"
[[ "${actual_lines}" -eq "${expected_lines}" ]] || fail "Expected ${expected_lines} code invocations, saw ${actual_lines}"

while IFS= read -r profile; do
  grep -q "action=open profile=${profile}$" "${CODE_LOG}" || fail "Missing log entry for profile ${profile}"
done < <(profiles)

log "install-extensions.sh honors --group AI and --without-group AI"
rm -f "${CODE_LOG}"
"${BASH}" "${ROOT}/scripts/install-extensions.sh" ai --group AI >/dev/null
logged_ai="$( { grep -c 'action=install' "${CODE_LOG}" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[[ "${logged_ai}" -eq 4 ]] || fail "Expected 4 AI suite installs for profile 'ai', saw ${logged_ai}"

rm -f "${CODE_LOG}"
"${BASH}" "${ROOT}/scripts/install-extensions.sh" ai --without-group AI >/dev/null
if grep -qE 'ext=(google\.google-antigravity|anthropic\.claude-code|openai\.chatgpt|github\.copilot-chat)$' "${CODE_LOG}"; then
  fail "--without-group AI still installed an AI extension"
fi

log "open-profiles.sh caches installs and invalidates when pin manifest changes"
rm -f "${CODE_LOG}"
"${BASH}" "${ROOT}/scripts/open-profiles.sh" ai >/dev/null
first_installs="$( { grep -c 'action=install' "${CODE_LOG}" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[[ "${first_installs}" -gt 0 ]] || fail "Expected initial install actions"

rm -f "${CODE_LOG}"
"${BASH}" "${ROOT}/scripts/open-profiles.sh" ai >/dev/null
second_installs="$( { grep -c 'action=install' "${CODE_LOG}" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[[ "${second_installs}" -eq 0 ]] || fail "Expected cache hit to skip installs; saw ${second_installs}"

# Mutate pin manifest (with empty extra key change) -> hash must invalidate cache.
printf '{"version":1,"pins":{},"comment":"bump"}\n' > "${ARTAGON_VSCODE_PINS_FILE}"
rm -f "${CODE_LOG}"
"${BASH}" "${ROOT}/scripts/open-profiles.sh" ai >/dev/null
third_installs="$( { grep -c 'action=install' "${CODE_LOG}" 2>/dev/null || echo 0; } | tr -d '[:space:]')"
[[ "${third_installs}" -gt 0 ]] || fail "Expected pin-manifest change to invalidate open-profiles cache"
printf '{"version":1,"pins":{}}\n' > "${ARTAGON_VSCODE_PINS_FILE}"

log "extension-id allowlist rejects injection-shaped values across install-extensions, import-profile, and vspcli"
INJ_ID='evil; rm -rf /tmp/x'
INJ_ROOT="${TMP}/inj-repo"
mkdir -p "${INJ_ROOT}/profiles/__inj__"
cp -R "${ROOT}/catalog" "${INJ_ROOT}/catalog"
printf '[{"identifier":{"id":"%s"}}]\n' "${INJ_ID}" > "${INJ_ROOT}/profiles/__inj__/extensions.json"

set +e
out_a="$(VSPCLI_ROOT="${INJ_ROOT}" "${BASH}" "${ROOT}/scripts/install-extensions.sh" "__inj__" 2>&1)"
rc_a=$?
set -e
[[ "${rc_a}" -ne 0 ]] || fail "install-extensions.sh accepted injection id"
printf '%s\n' "${out_a}" | grep -q 'rejected extension id' || fail "Missing 'rejected extension id' in install-extensions.sh output: ${out_a}"

INJ_BUNDLE="${TMP}/injection.code-profile"
printf '{"settings":{},"extensions":{"enabled":["%s"]}}\n' "${INJ_ID}" > "${INJ_BUNDLE}"
set +e
out_b="$("${BASH}" "${ROOT}/scripts/import-profile.sh" "inj-prof" "${INJ_BUNDLE}" 2>&1)"
rc_b=$?
set -e
[[ "${rc_b}" -ne 0 ]] || fail "import-profile.sh accepted injection id"
printf '%s\n' "${out_b}" | grep -q 'rejected extension id' || fail "Missing 'rejected extension id' in import-profile.sh output: ${out_b}"

set +e
out_c="$("${BASH}" "${ROOT}/scripts/vspcli" --install-ext "ai" "${INJ_ID}" 2>&1)"
rc_c=$?
set -e
[[ "${rc_c}" -ne 0 ]] || fail "vspcli --install-ext accepted injection id"
printf '%s\n' "${out_c}" | grep -q 'rejected extension id' || fail "Missing 'rejected extension id' in vspcli output: ${out_c}"

log "rand_hex_8 pure POSIX dd+od fallback works when openssl fails (zero Python)"
STUB_OPENSSL="${TMP}/stub-openssl"
mkdir -p "${STUB_OPENSSL}"
cat > "${STUB_OPENSSL}/openssl" <<'EOF'
#!/bin/sh
exit 1
EOF
chmod 0755 "${STUB_OPENSSL}/openssl"
out_hex="$(PATH="${STUB_OPENSSL}:${PATH}" "${BASH}" -c "source '${ROOT}/scripts/lib/common.sh'; rand_hex_8")"
[[ "${out_hex}" =~ ^[0-9a-f]{8}$ ]] || fail "Expected 8-hex fallback from rand_hex_8, got: ${out_hex}"

log "every merged profile satisfies protected VS Code 1.147.0 security/editor invariants"
for merged in "${ROOT}"/_merged/*.json; do
  jq -e '
    ."security.workspace.trust.untrustedFiles" == "prompt"
    and ."workbench.sideBar.location" == "left"
    and ."telemetry.telemetryLevel" == "off"
    and ."editor.hover.enabled" == "on"
  ' "${merged}" >/dev/null || fail "Protected baseline invariant violated in ${merged}"
done

log "outer sentinel profile and cache file preserved untouched"
[[ "$(jq -r '.sentinel' "${VSCODE_USER_DIR}/profiles/sentinel00/settings.json")" == "true" ]] \
  || fail "Outer sentinel profile settings.json was clobbered"
[[ "$(cat "${VSCODE_EXT_CACHE_DIR}/__sentinel__")" == "outer-cache-sentinel" ]] \
  || fail "Outer cache sentinel was clobbered"

log "All smoke tests passed."

log "==> Running BATS suite (scripts/tests/bats/*.bats)..."
if ! command -v bats >/dev/null 2>&1; then
  fail "bats is required on PATH (install: brew install bats-core or apt-get install bats)"
fi

bats_files=("${ROOT}"/scripts/tests/bats/*.bats)
[[ "${#bats_files[@]}" -gt 0 ]] || fail "no BATS files found under scripts/tests/bats/"

if ! bats "${bats_files[@]}"; then
  fail "BATS suite failed"
fi

log "All BATS tests passed."
