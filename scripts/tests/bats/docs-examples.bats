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

@test "docs: v3.0 README.md Quick Start and CLI commands pass flag parsing cleanly on a clean checkout" {
  require_bin jq
  install_code_mock "${TEST_TMP}/bin"
  install_empty_marketplace_curl_mock "${TEST_TMP}/mp-bin"

  # Never document unsupported --ux flag on import-profile.sh/vspcli import, retired v2 script invocations,
  # or dead raw-branch exports/*.code-profile markdown links.
  ! grep -E 'import-profile\.sh.*--ux|vspcli[[:space:]]+import.*--ux' "${REPO_ROOT}/README.md"
  ! grep -Eq 'bash[[:space:]]+scripts/(setup-rust-toolchain|setup-cpp-toolchain)\.sh|source[[:space:]]+scripts/\.vspcli-completion\.' "${REPO_ROOT}/README.md"
  ! grep -Eq '\]\(exports/[^)]+\.code-profile\)' "${REPO_ROOT}/README.md"

  # v3.0 README uses native Packs matrix headers and documents core unified `vsp` commands.
  grep -Fq '| Profile | Description | Included Packs | Extensions | Bundle Artifact |' "${REPO_ROOT}/README.md"
  grep -Fq '### Packs (`packs/`)' "${REPO_ROOT}/README.md"
  for expected_cmd in "vsp --version" "vsp list" "vsp show" "vsp detect" "vsp diff" "vsp sync" "vsp use" "vsp open" "vsp doctor"; do
    grep -Fq "${expected_cmd}" "${REPO_ROOT}/README.md"
  done

  # Prepare realistic temporary workspaces for documented /path/to/* placeholders.
  ws_repo="$(make_workspace doc-repo Cargo.toml)"
  ws_mvn="$(make_workspace doc-mvn pom.xml)"
  ws_astro="$(make_workspace doc-astro package.json astro.config.mjs)"
  ws_py="$(make_workspace doc-py pyproject.toml)"

  # Extract every `vsp ...` command from fenced ```bash blocks in README.md.
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
      if (line ~ /^vsp[[:space:]]/) print line
    }
  ' "${REPO_ROOT}/README.md" > "${cmds_file}"

  cmd_count="$(wc -l < "${cmds_file}" | tr -d ' ')"
  [[ "${cmd_count}" -ge 8 ]]

  # Provide an offline `vsp` shim on PATH using bin/vsp Bash fallback so tests run identically
  # on clean checkouts with zero tracked exports/ and zero pre-built target/ binary.
  vsp_shim_bin="${TEST_TMP}/vsp-shim-bin"
  mkdir -p "${vsp_shim_bin}"
  cat > "${vsp_shim_bin}/vsp" <<EOF
#!/usr/bin/env bash
unset VSP_BIN
export VSP_FORCE_BASH=1
exec "${REPO_ROOT}/bin/vsp" "\$@"
EOF
  chmod 0755 "${vsp_shim_bin}/vsp"

  # Run commands inside an isolated temp workdir so relative outputs (e.g. ./bundles) never dirty REPO_ROOT.
  doc_work="${TEST_TMP}/doc-workdir"
  mkdir -p "${doc_work}"

  while IFS= read -r cmd; do
    [[ -n "${cmd}" ]] || continue
    cmd="${cmd//\/path\/to\/maven-spring-repo/${ws_mvn}}"
    cmd="${cmd//\/path\/to\/astro-repo/${ws_astro}}"
    cmd="${cmd//\/path\/to\/python-repo/${ws_py}}"
    cmd="${cmd//\/path\/to\/repo/${ws_repo}}"

    set +e
    (cd "${doc_work}" && PATH="${vsp_shim_bin}:${PATH}" eval "${cmd}") >/dev/null 2>&1
    rc=$?
    set -e

    # No documented command may fail flag/argument validation (exit 4 or 2).
    if [[ "${rc}" -eq 4 || "${rc}" -eq 2 ]]; then
      echo "README command failed flag/arg validation (exit=${rc}): ${cmd}" >&2
      return 1
    fi
    # Core read/sync/export/open/use examples must succeed cleanly offline (doctor --strict may exit 1 on clean Default profile).
    case "${cmd}" in
      "vsp --version"|"vsp list "*|"vsp show "*|"vsp detect "*|"vsp diff "*|"vsp sync "*|"vsp open "*|"vsp use "*|"vsp export "*)
        if [[ "${rc}" -ne 0 ]]; then
          echo "README vsp command failed (exit=${rc}): ${cmd}" >&2
          return 1
        fi
        ;;
    esac
  done < "${cmds_file}"

  # Confirm documented workspace sync, profile sync, and open examples produced expected results.
  [[ -f "${ws_repo}/.vscode/settings.json" ]]
  rust_loc="$(jq -r '.userDataProfiles[] | select(.name == "rust") | .location' "${VSCODE_USER_DIR}/globalStorage/storage.json")"
  [[ -n "${rust_loc}" ]]
  [[ -f "${VSCODE_USER_DIR}/profiles/${rust_loc}/settings.json" ]]
  grep -q 'action=open profile=rust' "${CODE_LOG}"

  # Verify legacy vspcli compatibility commands and check-extension-compatibility.sh cleanly on clean checkout.
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
