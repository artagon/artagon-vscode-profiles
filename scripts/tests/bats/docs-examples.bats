#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

install_empty_marketplace_curl_mock() {
  local bin_dir="${1}"
  mkdir -p "${bin_dir}"
  cat > "${bin_dir}/curl" <<'MOCK'
#!/bin/sh
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    *)  shift ;;
  esac
done
if [ -n "${out}" ]; then
  printf '{"results":[{"extensions":[]}]}\n' > "${out}"
else
  printf '{"results":[{"extensions":[]}]}\n'
fi
MOCK
  chmod 0755 "${bin_dir}/curl"
  export PATH="${bin_dir}:${PATH}"
}

@test "docs: README.md documents valid 2-step UX import flow and all Quick Start / CLI commands pass flag parsing" {
  require_bin jq
  install_code_mock "${TEST_TMP}/bin"
  install_empty_marketplace_curl_mock "${TEST_TMP}/mp-bin"

  # Never document unsupported --ux flag on import-profile.sh or vspcli import.
  ! grep -E 'import-profile\.sh.*--ux|vspcli[[:space:]]+import.*--ux' "${REPO_ROOT}/README.md"
  grep -Fq 'bash scripts/vspcli apply --target=profile --profile rust --ux=retina' "${REPO_ROOT}/README.md"

  # Prepare realistic temporary workspaces for documented /path/to/* placeholders.
  ws_repo="$(make_workspace doc-repo Cargo.toml)"
  ws_mvn="$(make_workspace doc-mvn pom.xml)"
  ws_astro="$(make_workspace doc-astro package.json astro.config.mjs)"
  ws_py="$(make_workspace doc-py pyproject.toml)"

  # Extract every `bash scripts/...` command from fenced ```bash blocks in README.md.
  cmds_file="${TEST_TMP}/readme-cmds.txt"
  awk '
    /^```bash/ { in_bash=1; next }
    /^```/     { in_bash=0; next }
    in_bash {
      sub(/^[[:space:]]+/, "")
      if ($0 ~ /^#/ || $0 == "") next
      if ($0 ~ /\\$/) {
        sub(/\\$/, "")
        buf = buf $0 " "
        next
      }
      line = buf $0
      buf = ""
      if (line ~ /^bash scripts\//) print line
    }
  ' "${REPO_ROOT}/README.md" > "${cmds_file}"

  cmd_count="$(wc -l < "${cmds_file}" | tr -d ' ')"
  [[ "${cmd_count}" -ge 15 ]]

  # Build exports inside an isolated temp copy so standalone BATS passes on a clean checkout with zero tracked exports/.
  doc_repo="${TEST_TMP}/doc-repo"
  mkdir -p "${doc_repo}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" \
        "${REPO_ROOT}/packs" "${REPO_ROOT}/scripts" "${REPO_ROOT}/docs" "${doc_repo}/"
  cp "${REPO_ROOT}/README.md" "${REPO_ROOT}/AGENTS.md" "${REPO_ROOT}/CLAUDE.md" \
     "${REPO_ROOT}/CODEX.md" "${REPO_ROOT}/GEMINI.md" "${REPO_ROOT}/presets.jsonc" \
     "${REPO_ROOT}/vsp.lock.json" "${doc_repo}/"
  VSPCLI_ROOT="${doc_repo}" "${BASH}" "${doc_repo}/scripts/vspcli" build >/dev/null

  while IFS= read -r cmd; do
    [[ -n "${cmd}" ]] || continue
    # Substitute placeholder paths with real temp workspaces.
    cmd="${cmd//\/path\/to\/maven-spring-repo/${ws_mvn}}"
    cmd="${cmd//\/path\/to\/astro-repo/${ws_astro}}"
    cmd="${cmd//\/path\/to\/python-repo/${ws_py}}"
    cmd="${cmd//\/path\/to\/repo/${ws_repo}}"

    set +e
    (cd "${doc_repo}" && VSPCLI_ROOT="${doc_repo}" eval "${cmd}") >/dev/null 2>&1
    rc=$?
    set -e

    # No documented command may fail flag parsing (exit 4 or 2).
    if [[ "${rc}" -eq 4 || "${rc}" -eq 2 ]]; then
      echo "README command failed flag/arg validation (exit=${rc}): ${cmd}" >&2
      return 1
    fi
    # Import and apply examples must succeed cleanly offline.
    case "${cmd}" in
      *"scripts/import-profile.sh "*|*"scripts/vspcli apply "*)
        if [[ "${rc}" -ne 0 ]]; then
          echo "README import/apply command failed (exit=${rc}): ${cmd}" >&2
          return 1
        fi
        ;;
    esac
  done < "${cmds_file}"

  # Confirm the 2-step flow actually applied Retina UX to the imported rust profile.
  rust_loc="$(jq -r '.userDataProfiles[] | select(.name == "rust") | .location' "${VSCODE_USER_DIR}/globalStorage/storage.json")"
  [[ -n "${rust_loc}" ]]
  [[ "$(jq -r '."workbench.colorTheme"' "${VSCODE_USER_DIR}/profiles/${rust_loc}/settings.json")" == "Catppuccin Mocha" ]]

  # Verify CLI Quick Reference table flags (list, detect --json, check-extension-compatibility.sh).
  run vspcli list
  [[ "${status}" -eq 0 ]]
  run vspcli detect --json "${ws_repo}"
  [[ "${status}" -eq 0 ]]
  set +e
  (cd "${REPO_ROOT}" && bash scripts/check-extension-compatibility.sh --all --marketplace-only --json) >/dev/null 2>&1
  compat_rc=$?
  set -e
  [[ "${compat_rc}" -ne 2 && "${compat_rc}" -ne 4 ]]
}

@test "docs: docs/EXTENSION_COMPATIBILITY.md options and commands match check-extension-compatibility.sh --help" {
  install_code_mock "${TEST_TMP}/bin"
  install_empty_marketplace_curl_mock "${TEST_TMP}/mp-bin"

  doc="${REPO_ROOT}/docs/EXTENSION_COMPATIBILITY.md"

  # Dead/nonexistent flags must not appear anywhere in docs/EXTENSION_COMPATIBILITY.md.
  ! grep -Eq '(^|[[:space:]])(--verbose|--no-cache|-v)([[:space:]]|$)' "${doc}"

  # Every option listed in the Options: block must exist in --help output.
  help_out="$("${BASH}" "${SCRIPTS_DIR}/check-extension-compatibility.sh" --help)"
  doc_flags="$(awk '
    /^Options:/ { in_opts=1; next }
    /^```/      { in_opts=0; next }
    in_opts {
      n = split($0, toks, /[[:space:],]+/)
      for (i = 1; i <= n; i++) {
        if (toks[i] ~ /^--?[a-zA-Z][-a-zA-Z0-9]*$/) print toks[i]
      }
    }
  ' "${doc}")"
  [[ -n "${doc_flags}" ]]
  while IFS= read -r flag; do
    [[ -n "${flag}" ]] || continue
    if [[ "${help_out}" != *"${flag}"* ]]; then
      echo "Documented option not present in check-extension-compatibility.sh --help: ${flag}" >&2
      return 1
    fi
  done <<< "${doc_flags}"

  # Extract every bash scripts/check-extension-compatibility.sh command in the guide and run it offline.
  cmds_file="${TEST_TMP}/compat-doc-cmds.txt"
  awk '
    /\\$/ {
      sub(/\\$/, "")
      buf = buf $0 " "
      next
    }
    {
      line = buf $0
      buf = ""
      pos = index(line, "bash scripts/check-extension-compatibility.sh")
      if (pos > 0) {
        cmd = substr(line, pos)
        sub(/[[:space:]]*\|\|.*$/, "", cmd)
        sub(/[[:space:]]*\|.*$/, "", cmd)
        sub(/"[[:space:]]*$/, "", cmd)
        print cmd
      }
    }
  ' "${doc}" > "${cmds_file}"

  cmd_count="$(wc -l < "${cmds_file}" | tr -d ' ')"
  [[ "${cmd_count}" -ge 8 ]]

  # Build profiles once in a temp repo copy so `--all` and `--extensions-file profiles/rust/extensions.json` run fast on a clean checkout.
  compat_repo="${TEST_TMP}/compat-doc-repo"
  mkdir -p "${compat_repo}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" "${REPO_ROOT}/scripts" "${compat_repo}/"
  VSPCLI_ROOT="${compat_repo}" "${BASH}" "${compat_repo}/scripts/compose-settings.sh" >/dev/null

  while IFS= read -r cmd; do
    [[ -n "${cmd}" ]] || continue
    set +e
    (cd "${compat_repo}" && VSPCLI_ROOT="${compat_repo}" eval "${cmd}") >/dev/null 2>&1
    rc=$?
    set -e
    if [[ "${rc}" -eq 2 || "${rc}" -eq 4 ]]; then
      echo "EXTENSION_COMPATIBILITY.md command failed flag/profile validation (exit=${rc}): ${cmd}" >&2
      return 1
    fi
  done < "${cmds_file}"
}

@test "apply-rust-policy: malformed markers fail nonzero and leave target file bytes untouched" {
  fix="${TEST_TMP}/rust-policy-fix"
  mkdir -p "${fix}/docs"
  cp "${REPO_ROOT}/docs/rust.md" "${fix}/docs/rust.md"
  cp "${REPO_ROOT}/CLAUDE.md" "${REPO_ROOT}/CODEX.md" "${REPO_ROOT}/GEMINI.md" "${fix}/"

  tail_marker="IMPORTANT-USER-TAIL-CONTENT-DO-NOT-DROP"

  # 1. Unmatched BEGIN marker (missing END)
  cat > "${fix}/AGENTS.md" <<EOF
# Header
<!-- BEGIN RUST-DOCS-POLICY -->
stale content
${tail_marker}
EOF
  cp "${fix}/AGENTS.md" "${TEST_TMP}/snap-unmatched-begin"
  VSPCLI_ROOT="${fix}" run --separate-stderr "${BASH}" "${LIB_DIR}/apply-rust-policy.sh"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"malformed"* ]]
  cmp -s "${TEST_TMP}/snap-unmatched-begin" "${fix}/AGENTS.md"
  VSPCLI_ROOT="${fix}" run --separate-stderr "${BASH}" "${LIB_DIR}/apply-rust-policy.sh" --check
  [[ "${status}" -ne 0 ]]
  cmp -s "${TEST_TMP}/snap-unmatched-begin" "${fix}/AGENTS.md"

  # 2. Orphan END marker (missing BEGIN)
  cat > "${fix}/AGENTS.md" <<EOF
# Header
<!-- END RUST-DOCS-POLICY -->
${tail_marker}
EOF
  cp "${fix}/AGENTS.md" "${TEST_TMP}/snap-orphan-end"
  VSPCLI_ROOT="${fix}" run --separate-stderr "${BASH}" "${LIB_DIR}/apply-rust-policy.sh"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"malformed"* ]]
  cmp -s "${TEST_TMP}/snap-orphan-end" "${fix}/AGENTS.md"

  # 3. Reversed markers (END before BEGIN)
  cat > "${fix}/AGENTS.md" <<EOF
# Header
<!-- END RUST-DOCS-POLICY -->
middle
<!-- BEGIN RUST-DOCS-POLICY -->
${tail_marker}
EOF
  cp "${fix}/AGENTS.md" "${TEST_TMP}/snap-reversed"
  VSPCLI_ROOT="${fix}" run --separate-stderr "${BASH}" "${LIB_DIR}/apply-rust-policy.sh"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"malformed"* ]]
  cmp -s "${TEST_TMP}/snap-reversed" "${fix}/AGENTS.md"

  # 4. Duplicate marker pairs
  cat > "${fix}/AGENTS.md" <<EOF
<!-- BEGIN RUST-DOCS-POLICY -->
a
<!-- END RUST-DOCS-POLICY -->
<!-- BEGIN RUST-DOCS-POLICY -->
b
<!-- END RUST-DOCS-POLICY -->
${tail_marker}
EOF
  cp "${fix}/AGENTS.md" "${TEST_TMP}/snap-duplicate"
  VSPCLI_ROOT="${fix}" run --separate-stderr "${BASH}" "${LIB_DIR}/apply-rust-policy.sh"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"malformed"* ]]
  cmp -s "${TEST_TMP}/snap-duplicate" "${fix}/AGENTS.md"
}

@test "setup-java-toolchain: preserves existing .vscode/tasks.json untouched and directs user to vspcli apply" {
  install_code_mock "${TEST_TMP}/bin"

  ws_existing="$(make_workspace java-existing-tasks build.gradle)"
  mkdir -p "${ws_existing}/.vscode"
  cat > "${ws_existing}/.vscode/tasks.json" <<'EOF'
{
  // Custom user task must never be overwritten
  "version": "2.0.0",
  "tasks": [
    { "label": "Custom: User Task", "type": "shell", "command": "./custom.sh" }
  ]
}
EOF
  cp "${ws_existing}/.vscode/tasks.json" "${TEST_TMP}/tasks-snap.json"

  run --separate-stderr "${BASH}" "${SCRIPTS_DIR}/setup-java-toolchain.sh" java-gradle "${ws_existing}"
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"refusing to overwrite"* ]]
  [[ "${stderr}" == *"vspcli apply"* ]]
  cmp -s "${TEST_TMP}/tasks-snap.json" "${ws_existing}/.vscode/tasks.json"

  # Clean workspace without existing tasks.json still copies tasks.example.json cleanly.
  ws_clean="$(make_workspace java-clean-tasks build.gradle)"
  run --separate-stderr "${BASH}" "${SCRIPTS_DIR}/setup-java-toolchain.sh" java-gradle "${ws_clean}"
  [[ "${status}" -eq 0 ]]
  cmp -s "${REPO_ROOT}/profiles/java-gradle/tasks.example.json" "${ws_clean}/.vscode/tasks.json"
}
