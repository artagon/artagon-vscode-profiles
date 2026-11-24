#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "resolver: transitive requires[] resolves core-editor -> ts-base -> astro in deterministic order" {
  source "${LIB_DIR}/module-resolver.sh"
  run resolve_modules "${REPO_ROOT}" --roots "astro"
  [[ "${status}" -eq 0 ]]
  # core-editor before ts-base before astro; defaultWith adds ts-eslint-prettier.
  lines_csv="$(printf '%s\n' "${output}" | paste -sd ',' -)"
  [[ "${lines_csv}" == *"core-editor"* ]]
  [[ "${lines_csv}" == *"ts-base"* ]]
  [[ "${lines_csv}" == *"ts-eslint-prettier"* ]]
  [[ "${lines_csv}" == *"astro"* ]]
  # Verify dependency ordering: ts-base precedes astro.
  idx_ts="$(printf '%s\n' "${output}" | grep -n '^ts-base$' | cut -d: -f1)"
  idx_astro="$(printf '%s\n' "${output}" | grep -n '^astro$' | cut -d: -f1)"
  [[ "${idx_ts}" -lt "${idx_astro}" ]]
}

@test "resolver: unified EXCLUDED suppresses defaultWith[] before conflict evaluation (astro + biome - ts-eslint-prettier)" {
  source "${LIB_DIR}/module-resolver.sh"
  run resolve_modules "${REPO_ROOT}" --roots $'astro\nbiome' --exclude "ts-eslint-prettier"
  [[ "${status}" -eq 0 ]]
  grep -Fxq "astro" <<<"${output}"
  grep -Fxq "biome" <<<"${output}"
  ! grep -Fxq "ts-eslint-prettier" <<<"${output}"
}

@test "resolver: conflicting modules return 4 without terminating sourced caller shell" {
  run "${BASH}" -c "
    source '${LIB_DIR}/module-resolver.sh'
    rc=0
    resolve_modules '${REPO_ROOT}' --roots \$'ai-agents\nai-copilot' >/dev/null 2>&1 || rc=\$?
    echo \"survived:rc=\${rc}\"
  "
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == "survived:rc=4" ]]
}

@test "resolver: rejects cycle, invalid module ID regex, and missing required dependency (return 4)" {
  fixture="${TEST_TMP}/bad-dag"
  mkdir -p "${fixture}/modules/capabilities/cyc-a" "${fixture}/modules/capabilities/cyc-b"
  printf '{"id":"cyc-a","kind":"capability","requires":["cyc-b"]}\n' > "${fixture}/modules/capabilities/cyc-a/module.json"
  printf '{"id":"cyc-b","kind":"capability","requires":["cyc-a"]}\n' > "${fixture}/modules/capabilities/cyc-b/module.json"

  source "${LIB_DIR}/module-resolver.sh"
  run resolve_modules "${fixture}" --roots "cyc-a"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"cycle"* ]]

  run resolve_modules "${REPO_ROOT}" --roots "../evil"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"invalid module id"* ]]

  # Excluding a hard-required dependency must fail with return 4.
  run resolve_modules "${REPO_ROOT}" --roots "rust" --exclude "core-editor"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"excluded"* ]]
}

@test "resolver: resolve_profile_recipe handles general-minimal (excludes ai-agents) and ai-copilot cleanly" {
  source "${LIB_DIR}/module-resolver.sh"
  run resolve_profile_recipe "${REPO_ROOT}" "general-minimal"
  [[ "${status}" -eq 0 ]]
  grep -Fxq "core-editor" <<<"${output}"
  ! grep -Fxq "ai-agents" <<<"${output}"
  ! grep -Fxq "ai-copilot" <<<"${output}"

  run resolve_profile_recipe "${REPO_ROOT}" "ai-copilot"
  [[ "${status}" -eq 0 ]]
  grep -Fxq "ai-copilot" <<<"${output}"
  ! grep -Fxq "ai-agents" <<<"${output}"
}
