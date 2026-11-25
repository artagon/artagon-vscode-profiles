#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "compiler: Astro ESLint+Prettier -> Biome switch removes stale Prettier/ESLint workspace settings keys" {
  ws="$(make_workspace astro-switch package.json astro.config.mjs)"

  # First apply: Astro defaults to ts-eslint-prettier.
  run vspcli apply --target=workspace --check-compat=off "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."[typescript]"."editor.defaultFormatter"' "${ws}/.vscode/settings.json")" == "esbenp.prettier-vscode" ]]
  [[ "$(jq -r '."[typescript]"."editor.codeActionsOnSave"."source.fixAll.eslint"' "${ws}/.vscode/settings.json")" == "explicit" ]]
  [[ "$(jq 'has("eslint.validate")' "${ws}/.vscode/settings.json")" == "true" ]]
  jq -e '.recommendations | index("esbenp.prettier-vscode") != null' "${ws}/.vscode/extensions.json"

  # Second apply: switch to Biome (--with biome --without ts-eslint-prettier).
  run vspcli apply --target=workspace --check-compat=off --with biome --without ts-eslint-prettier "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."[typescript]"."editor.defaultFormatter"' "${ws}/.vscode/settings.json")" == "biomejs.biome" ]]
  [[ "$(jq 'has("eslint.validate")' "${ws}/.vscode/settings.json")" == "false" ]]
  [[ "$(jq '."[typescript]"."editor.codeActionsOnSave" | has("source.fixAll.eslint")' "${ws}/.vscode/settings.json")" == "false" ]]
  [[ "$(jq -r '."[typescript]"."editor.codeActionsOnSave"."source.fixAll.biome"' "${ws}/.vscode/settings.json")" == "explicit" ]]
  jq -e '.recommendations | index("biomejs.biome") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("esbenp.prettier-vscode") == null' "${ws}/.vscode/extensions.json"
}

@test "compiler: general-minimal sets chat.disableAIFeatures=true; returning to AI-enabled profile clears it" {
  install_code_mock "${TEST_TMP}/bin"

  # Step 1: apply general-minimal to a named VS Code profile.
  run vspcli apply --target=profile --profile dev-test --toolchain general-minimal
  [[ "${status}" -eq 0 ]]
  prof_settings="$(find "${VSCODE_USER_DIR}/profiles" -name settings.json | head -n 1)"
  [[ -f "${prof_settings}" ]]
  [[ "$(jq -r '."chat.disableAIFeatures"' "${prof_settings}")" == "true" ]]

  # Step 2: switch same profile to AI-enabled 'rust' -> chat.disableAIFeatures MUST be removed!
  run vspcli apply --target=profile --profile dev-test --toolchain rust
  [[ "${status}" -eq 0 ]]
  [[ "$(jq 'has("chat.disableAIFeatures")' "${prof_settings}")" == "false" ]]
  [[ "$(jq -r '."chat.agent.enabled"' "${prof_settings}")" == "true" ]]
}

@test "compiler: Python workspace uses portable venv defaults (no uv machine-scope key); profile gets alwaysUseUv=true" {
  ws="$(make_workspace py-ws pyproject.toml)"
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."python.languageServer"' "${ws}/.vscode/settings.json")" == "None" ]]
  [[ "$(jq -r '."[python]"."editor.tabSize"' "${ws}/.vscode/settings.json")" -eq 4 ]]
  [[ "$(jq -r '."python-envs.defaultEnvManager"' "${ws}/.vscode/settings.json")" == "ms-python.python:venv" ]]
  # Machine-scoped python-envs.alwaysUseUv must NOT appear in workspace settings.
  [[ "$(jq 'has("python-envs.alwaysUseUv")' "${ws}/.vscode/settings.json")" == "false" ]]

  # Compiled python profile DOES contain machine-scoped python-envs.alwaysUseUv=true.
  py_merged="${TEST_TMP}/python.json"
  source "${LIB_DIR}/common.sh"
  source "${LIB_DIR}/module-resolver.sh"
  source "${LIB_DIR}/compiler.sh"
  compiler_compile_profile_settings_file "${REPO_ROOT}" python "${py_merged}"
  [[ "$(jq -r '."python-envs.alwaysUseUv"' "${py_merged}")" == "true" ]]
}

@test "compiler: jq 1.7.1 scalar_leaves detects object-vs-leaf prefix collision across jointly selectable modules" {
  fixture="${TEST_TMP}/collision-repo"
  mkdir -p "${fixture}/catalog" "${fixture}/modules/capabilities/mod-a" "${fixture}/modules/capabilities/mod-b" "${fixture}/profiles"
  cp "${REPO_ROOT}/catalog/"*.json "${fixture}/catalog/"
  cp -R "${REPO_ROOT}/modules/capabilities/core-editor" "${fixture}/modules/capabilities/"
  cp -R "${REPO_ROOT}/modules/ux" "${fixture}/modules/"

  printf '{"id":"mod-a","kind":"capability","settings":{"workspace":"settings.workspace.jsonc"}}\n' > "${fixture}/modules/capabilities/mod-a/module.json"
  printf '{"custom.ns":{"sub":1}}\n' > "${fixture}/modules/capabilities/mod-a/settings.workspace.jsonc"

  printf '{"id":"mod-b","kind":"capability","settings":{"workspace":"settings.workspace.jsonc"}}\n' > "${fixture}/modules/capabilities/mod-b/module.json"
  printf '{"custom.ns":"scalar-leaf"}\n' > "${fixture}/modules/capabilities/mod-b/settings.workspace.jsonc"

  run env VSPCLI_ROOT="${fixture}" "${BASH}" "${SCRIPTS_DIR}/validate-json.sh"
  [[ "${status}" -eq 1 ]]
  [[ "${output}" == *"object-vs-leaf collision"* ]]
  [[ "${output}" == *"custom.ns"* ]]
}

@test "compiler: Java Maven + Spring layers cleanly without conflict and preserves machine-scoped gradle args in profile only" {
  ws="$(make_workspace mvn-spring pom.xml)"
  mkdir -p "${ws}/src/main/java"
  printf '@SpringBootApplication\nclass App {}\n' > "${ws}/src/main/java/AppApplication.java"

  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  jq -e '.recommendations | index("vscjava.vscode-maven") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("vmware.vscode-spring-boot") != null' "${ws}/.vscode/extensions.json"
  [[ "$(jq -r '."java.jdt.ls.lombokSupport.enabled"' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq 'has("java.import.gradle.arguments")' "${ws}/.vscode/settings.json")" == "false" ]]
  jg_merged="${TEST_TMP}/java-gradle.json"
  source "${LIB_DIR}/common.sh"
  source "${LIB_DIR}/module-resolver.sh"
  source "${LIB_DIR}/compiler.sh"
  compiler_compile_profile_settings_file "${REPO_ROOT}" java-gradle "${jg_merged}"
  [[ "$(jq -r '."java.import.gradle.arguments"' "${jg_merged}")" == *"--parallel"* ]]
  [[ "$(jq -r '."java.import.gradle.jvmArguments"' "${jg_merged}")" == *"-Xmx2G"* ]]
  [[ "$(jq 'has("java.import.gradle.jvmArguments")' "${ws}/.vscode/settings.json")" == "false" ]]
  [[ "$(jq -r '."files.associations"."application*.yml"' "${ws}/.vscode/settings.json")" == "spring-boot-properties-yaml" ]]
}

@test "compiler: Project Explorer nesting, Antigravity default AI, Markdown hard breaks, and GitHub Actions YAML handoff compile cleanly" {
  built="${TEST_TMP}/compiled-repo"
  mkdir -p "${built}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" "${built}/"
  VSPCLI_ROOT="${built}" "${BASH}" "${SCRIPTS_DIR}/compose-settings.sh" >/dev/null

  # Compiled AI profile includes Project Explorer file nesting, completion ergonomics, and Antigravity default settings.
  [[ "$(jq -r '."explorer.fileNesting.enabled"' "${built}/_merged/ai.json")" == "true" ]]
  [[ "$(jq -r '."explorer.fileNesting.patterns"."Cargo.toml"' "${built}/_merged/ai.json")" == *"Cargo.lock"* ]]
  [[ "$(jq -r '."editor.suggestSelection"' "${built}/_merged/ai.json")" == "recentlyUsedByPrefix" ]]
  [[ "$(jq -r '."antigravity.autoAcceptOnChat"' "${built}/_merged/ai.json")" == "false" ]]
  [[ "$(jq -r '."github.copilot.enable"."markdown"' "${built}/_merged/ai.json")" == "false" ]]
  [[ "$(jq -r '."[markdown]"."files.trimTrailingWhitespace"' "${built}/_merged/ai.json")" == "false" ]]

  # Rust profile uses isolated RA targetDir and stable formatOnSaveMode.
  [[ "$(jq -r '."rust-analyzer.cargo.targetDir"' "${built}/_merged/rust.json")" == "true" ]]
  [[ "$(jq -r '."[rust]"."editor.formatOnSaveMode"' "${built}/_merged/rust.json")" == "file" ]]

  # GitHub workflows profile lets github-actions-workflow own workflows without files.associations override.
  [[ "$(jq 'has("files.associations")' "${REPO_ROOT}/modules/capabilities/github-workflows/settings.workspace.jsonc" 2>/dev/null || echo false)" == "false" ]]
  [[ "$(jq -r '."[github-actions-workflow]"."editor.defaultFormatter"' "${built}/_merged/github-workflows.json")" == "redhat.vscode-yaml" ]]

  # Tier 1 IntelliJ-style DX settings + dx-pro in ai-plus + opt-in intellij-keybindings resolve cleanly.
  [[ "$(jq -r '."editor.suggest.insertMode"' "${built}/_merged/ai.json")" == "replace" ]]
  [[ "$(jq -r '."editor.suggest.matchOnWordStartOnly"' "${built}/_merged/ai.json")" == "false" ]]
  [[ "$(jq -r '."editor.occurrencesHighlight"' "${built}/_merged/ai.json")" == "multiFile" ]]
  [[ "$(jq -r '."debug.inlineValues"' "${built}/_merged/ai.json")" == "on" ]]
  [[ "$(jq -r '."workbench.editor.limit.value"' "${built}/_merged/ai.json")" -eq 10 ]]
  [[ "$(jq -r '."bookmarks.navigateThroughAllFiles"' "${built}/_merged/ai-plus.json")" == "true" ]]
  jq -e '.[] | select(.identifier.id == "buenon.scratchpads")' "${built}/profiles/ai-plus/extensions.json" >/dev/null
  jq -e '.[] | select(.identifier.id == "chadalen.vscode-jetbrains-icon-theme")' "${built}/profiles/ai-plus/extensions.json" >/dev/null
  # intellij-keybindings stays opt-in (absent from default ai-plus unless --with intellij-keybindings).
  ! jq -e '.[] | select(.identifier.id == "k--kato.intellij-idea-keybindings")' "${built}/profiles/ai-plus/extensions.json" >/dev/null

  # Every one of the 15 compiled profiles includes shell LSP/linter/formatter + JSON/YAML/TOML/XML/CSV tooling.
  merged_count=0
  for merged in "${built}"/_merged/*.json; do
    [[ -f "${merged}" ]] || continue
    merged_count=$((merged_count + 1))
    p="$(basename "${merged}" .json)"
    [[ "$(jq -r '."bashIde.includeAllWorkspaceSymbols"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."shellcheck.enable"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."json.validate.enable"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."yaml.validate"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."yaml.schemaStore.enable"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."evenBetterToml.schema.enabled"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."xml.server.preferBinary"' "${merged}")" == "true" ]]
    [[ "$(jq -r '."redhat.telemetry.enabled"' "${merged}")" == "false" ]]
    [[ "$(jq 'has("[json]") and has("[jsonc]") and has("[yaml]") and has("[toml]") and has("[xml]")' "${merged}")" == "true" ]]
    jq -e '.[] | select(.identifier.id == "mads-hartmann.bash-ide-vscode")' "${built}/profiles/${p}/extensions.json" >/dev/null
    jq -e '.[] | select(.identifier.id == "timonwong.shellcheck")' "${built}/profiles/${p}/extensions.json" >/dev/null
    jq -e '.[] | select(.identifier.id == "mkhl.shfmt")' "${built}/profiles/${p}/extensions.json" >/dev/null
    jq -e '.[] | select(.identifier.id == "redhat.vscode-yaml")' "${built}/profiles/${p}/extensions.json" >/dev/null
    jq -e '.[] | select(.identifier.id == "redhat.vscode-xml")' "${built}/profiles/${p}/extensions.json" >/dev/null
    jq -e '.[] | select(.identifier.id == "mechatroner.rainbow-csv")' "${built}/profiles/${p}/extensions.json" >/dev/null
  done
  [[ "${merged_count}" -eq 15 ]]
}

@test "compiler: workspace fingerprint is stable across both GNU stat (-c %i) and BSD stat (-f %i)" {
  ws="$(make_workspace fp-stat Cargo.toml)"
  fake_gnu_bin="${TEST_TMP}/fake-gnu-stat"
  mkdir -p "${fake_gnu_bin}"
  counter_file="${TEST_TMP}/stat-counter"
  printf '0\n' > "${counter_file}"
  # Simulate GNU coreutils stat: -c '%i' returns decimal inode, while -f treats '%i' as a missing
  # path AND prints changing filesystem free-block output for the second arg before exiting 1.
  cat > "${fake_gnu_bin}/stat" <<EOF
#!/bin/sh
if [ "\${1:-}" = "-c" ] && [ "\${2:-}" = "%i" ]; then
  printf '424242\n'
  exit 0
fi
if [ "\${1:-}" = "-f" ]; then
  n=\$(cat "${counter_file}")
  n=\$((n + 1))
  printf '%s\n' "\$n" > "${counter_file}"
  printf '  File: "%s"\nBlocks: Free: %s\n' "\${3:-}" "\$n"
  echo "stat: cannot read file system information for '\${2:-}': No such file or directory" >&2
  exit 1
fi
exit 1
EOF
  chmod 0755 "${fake_gnu_bin}/stat"

  # Under simulated GNU stat, fingerprint must be stable across repeated calls and match sha256(ws|424242).
  run env PATH="${fake_gnu_bin}:${PATH}" bash -c '
    set -euo pipefail
    source "'"${LIB_DIR}"'/common.sh"
    source "'"${LIB_DIR}"'/compiler.sh"
    fp1="$(compiler_workspace_fingerprint "'"${ws}"'")"
    fp2="$(compiler_workspace_fingerprint "'"${ws}"'")"
    expected="$(printf "%s|424242" "'"${ws}"'" | sha256_for_file -)"
    [[ "${fp1}" == "${fp2}" && "${fp1}" == "${expected}" ]]
  '
  [[ "${status}" -eq 0 ]]
}

@test "compiler: compiler_build_profile propagates ln -sfn failure and cleans up temp dir" {
  fixture="${TEST_TMP}/ln-fail-repo"
  mkdir -p "${fixture}/profiles/rust"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${fixture}/"
  cp "${REPO_ROOT}/profiles/rust/profile.json" "${fixture}/profiles/rust/"

  fail_bin="${TEST_TMP}/fail-ln-bin"
  mkdir -p "${fail_bin}"
  cat > "${fail_bin}/ln" <<'EOF'
#!/bin/sh
echo "ln: simulated symlink failure" >&2
exit 1
EOF
  chmod 0755 "${fail_bin}/ln"

  build_tmp="${TEST_TMP}/build-tmp"
  mkdir -p "${build_tmp}"

  # Deliberately run WITHOUT set -e so compiler_build_profile must explicitly check and propagate ln -sfn failure.
  run --separate-stderr env TMPDIR="${build_tmp}" PATH="${fail_bin}:${PATH}" "${BASH}" -c "
    source '${LIB_DIR}/common.sh'
    source '${LIB_DIR}/module-resolver.sh'
    source '${LIB_DIR}/compiler.sh'
    compiler_build_profile '${fixture}' rust
  "
  [[ "${status}" -ne 0 ]]
  [[ "${stderr}" == *"failed to symlink"* ]]
  leftover="$(find "${build_tmp}" -mindepth 1 -maxdepth 1 -name 'vsp-build-prof.*' | wc -l | tr -d ' ')"
  [[ "${leftover}" -eq 0 ]]
}

@test "compiler: A19 export-profiles and install-extensions compile fresh profile.json instead of reusing stale _merged/extensions.json" {
  repo="${TEST_TMP}/a19-stale-repo"
  mkdir -p "${repo}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" "${repo}/"

  # Initial build populates gitignored _merged/rust.json and profiles/rust/extensions.json.
  VSPCLI_ROOT="${repo}" "${BASH}" "${SCRIPTS_DIR}/compose-settings.sh" rust >/dev/null
  [[ -f "${repo}/_merged/rust.json" ]]
  [[ -f "${repo}/profiles/rust/extensions.json" ]]

  # Edit authoritative profiles/rust/profile.json AFTER build (without re-running compose-settings.sh).
  jq '.modules = ((.modules // []) + ["dx-pro"]) | .overrides.profile = ((.overrides.profile // {}) + {"editor.rulers": [111]})' \
    "${repo}/profiles/rust/profile.json" > "${repo}/profiles/rust/profile.json.tmp"
  mv -f "${repo}/profiles/rust/profile.json.tmp" "${repo}/profiles/rust/profile.json"

  # Verify stale artifacts on disk do NOT contain the new edits.
  [[ "$(jq 'has("editor.rulers")' "${repo}/_merged/rust.json")" == "false" ]]
  ! jq -e '.[] | select(.identifier.id == "alefragnani.bookmarks")' "${repo}/profiles/rust/extensions.json" >/dev/null

  # export-profiles.sh must compile fresh from profile.json (ignoring stale _merged/ and extensions.json).
  run env VSPCLI_ROOT="${repo}" "${BASH}" "${SCRIPTS_DIR}/export-profiles.sh" rust
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -c '.settings."editor.rulers"' "${repo}/exports/rust.code-profile")" == "[111]" ]]
  jq -e '.extensions.enabled | index("alefragnani.bookmarks") != null' "${repo}/exports/rust.code-profile" >/dev/null

  # install-extensions.sh must also compile fresh from profile.json (ignoring stale extensions.json).
  run env VSPCLI_ROOT="${repo}" "${BASH}" "${SCRIPTS_DIR}/install-extensions.sh" rust --dry-run
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"alefragnani.bookmarks"* ]]
}

@test "compiler: A22 sourced compiler.sh helpers propagate upstream compile/jq failures with set +e/+o pipefail without mutating caller options" {
  repo="${TEST_TMP}/a22-fail-repo"
  mkdir -p "${repo}"
  cp -R "${REPO_ROOT}/catalog" "${REPO_ROOT}/modules" "${REPO_ROOT}/profiles" "${repo}/"

  # Break rust settings JSONC so upstream compiler_compile_settings fails.
  printf '{broken jsonc\n' > "${repo}/modules/toolchains/rust/settings.profile.jsonc"

  run "${BASH}" -c '
    set +e
    set +o pipefail
    IFS=$'\'' \t\n'\''
    trap '\''true'\'' USR1

    snap_opts() {
      printf "FLAGS=%s\n" "$-"
      shopt -p
      printf "IFS_HEX=%s\n" "$(printf "%s" "${IFS}" | od -An -tx1 | tr -d "[:space:]")"
      trap -p
    }

    before="$(snap_opts)"

    source "'"${LIB_DIR}"'/common.sh"
    source "'"${LIB_DIR}"'/module-resolver.sh"
    source "'"${LIB_DIR}"'/compiler.sh"

    out1="'"${TEST_TMP}"'/a22-settings.json"
    compiler_compile_profile_settings_file "'"${repo}"'" rust "${out1}" >/dev/null 2>&1
    rc_settings=$?

    # Break rust module.json so upstream compiler_compile_extensions fails.
    printf "{broken module\n" > "'"${repo}"'/modules/toolchains/rust/module.json"
    out2="'"${TEST_TMP}"'/a22-exts.json"
    compiler_compile_profile_extensions_file "'"${repo}"'" rust "${out2}" >/dev/null 2>&1
    rc_exts=$?

    compiler_build_profile "'"${repo}"'" rust >/dev/null 2>&1
    rc_build=$?

    # Break crisp UX settings so compiler_ux_managed_keys_json fails.
    printf "{broken ux\n" > "'"${repo}"'/modules/ux/crisp/settings.profile.jsonc"
    compiler_ux_managed_keys_json "'"${repo}"'" >/dev/null 2>&1
    rc_ux=$?

    after="$(snap_opts)"

    echo "rc_settings=${rc_settings} rc_exts=${rc_exts} rc_build=${rc_build} rc_ux=${rc_ux}"
    if [[ "${rc_settings}" -eq 0 || "${rc_exts}" -eq 0 || "${rc_build}" -eq 0 || "${rc_ux}" -eq 0 ]]; then
      exit 1
    fi
    if [[ "${before}" != "${after}" ]]; then
      echo "CALLER SHELL OPTIONS MUTATED" >&2
      diff -u <(printf "%s\n" "${before}") <(printf "%s\n" "${after}") >&2 || true
      exit 2
    fi
    exit 0
  '
  [[ "${status}" -eq 0 ]]
}
