#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "workspace: Rust-only emits exact parsed workspace recommendations, jscearcy unwanted, and zero AI/UX leakage" {
  require_bin jq
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ -f "${ws}/.vscode/extensions.json" ]]
  [[ -f "${ws}/.vscode/settings.json" ]]

  # Exact workspace recommendations: core-editor (4) + markdown (4) + shell (6) + rust (4) = 18
  expected_recs='["bierner.markdown-mermaid","bierner.markdown-preview-github-styles","bmalehorn.vscode-fish","davidanson.vscode-markdownlint","dotenv.dotenv-vscode","editorconfig.editorconfig","fill-labs.dependi","jeff-hykin.better-shellscript-syntax","kherring.bats-test-runner","mads-hartmann.bash-ide-vscode","mkhl.shfmt","redhat.vscode-yaml","rust-lang.rust-analyzer","ryanluker.vscode-coverage-gutters","tamasfe.even-better-toml","timonwong.shellcheck","vadimcn.vscode-lldb","yzhang.markdown-all-in-one"]'
  [[ "$(jq -S -c '.recommendations | sort' "${ws}/.vscode/extensions.json")" == "${expected_recs}" ]]

  # jscearcy.rust-doc-viewer is in unwantedRecommendations; github.copilot (profile-only ai-agents) is NOT leaked!
  expected_unwanted='["barbosshack.crates-io","dustypomerleau.rust-syntax","foxundermoon.shell-format","jetmartin.bats","jscearcy.rust-doc-viewer","mikestead.dotenv","panicbit.cargo","serayuzgur.crates","shd101wyy.markdown-preview-enhanced"]'
  [[ "$(jq -S -c '.unwantedRecommendations | sort' "${ws}/.vscode/extensions.json")" == "${expected_unwanted}" ]]

  # Zero AI extensions and zero personal UX keys in workspace files.
  ! grep -qiE 'claude-code|openai\.chatgpt|copilot|antigravity|gemini|tokyo-night|catppuccin|fluent-icons|material-product-icons' "${ws}/.vscode/extensions.json"
  [[ "$(jq 'has("workbench.colorTheme") or has("workbench.iconTheme") or has("workbench.productIconTheme") or has("editor.fontFamily")' "${ws}/.vscode/settings.json")" == "false" ]]
}

@test "workspace: Rust + .github/workflows stacks github-workflows additively" {
  require_bin jq
  ws="$(make_workspace rust-gha Cargo.toml)"
  mkdir -p "${ws}/.github/workflows"
  : > "${ws}/.github/workflows/ci.yml"

  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  jq -e '.recommendations | index("rust-lang.rust-analyzer") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("github.vscode-github-actions") != null' "${ws}/.vscode/extensions.json"
}

@test "workspace: workflow-only repo emits github.vscode-github-actions without language toolchains" {
  require_bin jq
  ws="$(make_workspace gha-only)"
  mkdir -p "${ws}/.github/workflows"
  : > "${ws}/.github/workflows/release.yaml"

  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  jq -e '.recommendations | index("github.vscode-github-actions") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("rust-lang.rust-analyzer") == null' "${ws}/.vscode/extensions.json"
}

@test "workspace: XDG_STATE_HOME provenance removes deselected managed IDs while preserving user-owned IDs" {
  require_bin jq
  ws="$(make_workspace rust-prov Cargo.toml)"
  mkdir -p "${ws}/.vscode"
  # User already owns timonwong.shellcheck and existing.foo BEFORE first vspcli run!
  cat > "${ws}/.vscode/extensions.json" <<'EOF'
{
  "recommendations": ["existing.foo", "timonwong.shellcheck"],
  "unwantedRecommendations": ["unwanted.bar"]
}
EOF

  # First apply includes shell (which also lists timonwong.shellcheck + mkhl.shfmt).
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  jq -e '.recommendations | index("mkhl.shfmt") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("timonwong.shellcheck") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("existing.foo") != null' "${ws}/.vscode/extensions.json"
  jq -e '.unwantedRecommendations | index("unwanted.bar") != null' "${ws}/.vscode/extensions.json"

  # Second apply excludes shell: vspcli-added mkhl.shfmt MUST be pruned, while user-owned timonwong.shellcheck + existing.foo MUST remain!
  run vspcli apply --target=workspace --without shell "${ws}"
  [[ "${status}" -eq 0 ]]
  jq -e '.recommendations | index("mkhl.shfmt") == null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("timonwong.shellcheck") != null' "${ws}/.vscode/extensions.json"
  jq -e '.recommendations | index("existing.foo") != null' "${ws}/.vscode/extensions.json"
}

@test "workspace: idempotent rerun produces identical recommendations and settings" {
  require_bin jq
  ws="$(make_workspace rust Cargo.toml)"
  vspcli apply --target=workspace "${ws}" >/dev/null
  snap1="$(jq -S . "${ws}/.vscode/extensions.json")"
  vspcli apply --target=workspace "${ws}" >/dev/null
  snap2="$(jq -S . "${ws}/.vscode/extensions.json")"
  [[ "${snap1}" == "${snap2}" ]]
}

@test "workspace: dry-run emits would-be writes without creating .vscode/" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --dry-run "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"DRY-RUN"* ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "workspace: default --dry-run skips VSPCLI_COMPAT_CHECKER while explicit --check-compat runs it" {
  ws="$(make_workspace rust-dry-compat Cargo.toml)"
  call_log="${TEST_TMP}/compat-calls.log"
  stub="${TEST_TMP}/compat-stub.sh"
  cat > "${stub}" <<EOF
#!/usr/bin/env bash
printf 'called:%s\n' "\$*" >> '${call_log}'
printf '{"vscode_version":"1.147.0","summary":{"total":1,"compatible":0,"incompatible":1,"not_found":0},"extensions":[{"id":"rust-lang.rust-analyzer","status":"incompatible","version":"0.4.0","engine":">=1.999.0","prerelease":false}]}\n'
exit 1
EOF
  chmod 0755 "${stub}"

  # Default workspace --dry-run makes 0 calls to checker and reports compatibility=unchecked.
  VSPCLI_COMPAT_CHECKER="${stub}" run "${BASH}" "${SCRIPTS_DIR}/install-workspace.sh" --target=workspace --dry-run "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"compatibility=unchecked"* ]]
  [[ ! -s "${call_log}" ]]
  [[ ! -d "${ws}/.vscode" ]]

  # Default profile --dry-run also makes 0 calls to checker and reports compatibility=unchecked.
  VSPCLI_COMPAT_CHECKER="${stub}" run "${BASH}" "${SCRIPTS_DIR}/install-workspace.sh" --target=profile --profile rust --dry-run
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"compatibility=unchecked"* ]]
  [[ ! -s "${call_log}" ]]

  # Explicit --check-compat=warn with --dry-run calls checker once and proceeds.
  VSPCLI_COMPAT_CHECKER="${stub}" run "${BASH}" "${SCRIPTS_DIR}/install-workspace.sh" --target=workspace --dry-run --check-compat=warn "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" != *"compatibility=unchecked"* ]]
  [[ "$(wc -l < "${call_log}" | tr -d ' ')" -eq 1 ]]
  [[ ! -d "${ws}/.vscode" ]]

  # Explicit --check-compat=block with --dry-run calls checker and aborts nonzero.
  VSPCLI_COMPAT_CHECKER="${stub}" run "${BASH}" "${SCRIPTS_DIR}/install-workspace.sh" --target=workspace --dry-run --check-compat=block "${ws}"
  [[ "${status}" -eq 1 ]]
  [[ "$(wc -l < "${call_log}" | tr -d ' ')" -eq 2 ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "workspace: --rtk-in-workspace guards against leaking literal HOME paths into .vscode/settings.json" {
  require_bin jq
  ws="$(make_workspace rust-rtk-home Cargo.toml)"

  # Case 1: XDG_CONFIG_HOME under $HOME (and ZDOTDIR under $HOME) -> warns and rewrites literal $HOME to ${userHome}.
  XDG_CONFIG_HOME="${HOME}/.config" ZDOTDIR="${HOME}/.zsh-custom" \
    run --separate-stderr vspcli apply --target=workspace --check-compat=off --rtk-in-workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"--rtk-in-workspace"* && "${stderr}" == *'${userHome}'* ]]
  ! grep -Fq "${HOME}" "${ws}/.vscode/settings.json"
  [[ "$(jq -r '."terminal.integrated.profiles.osx"."rtk-zsh".env.ZDOTDIR' "${ws}/.vscode/settings.json")" == '${userHome}/.config/artagon-vscode/rtk/zsh' ]]
  [[ "$(jq -r '."terminal.integrated.profiles.osx"."rtk-zsh".env.ARTAGON_ORIG_ZDOTDIR' "${ws}/.vscode/settings.json")" == '${userHome}/.zsh-custom' ]]
  [[ "$(jq -r '."terminal.integrated.profiles.osx"."rtk-fish".env.ARTAGON_RTK_DIR' "${ws}/.vscode/settings.json")" == '${userHome}/.config/artagon-vscode/rtk' ]]
  [[ "$(jq -r '."terminal.integrated.profiles.osx"."rtk-bash".args[1]' "${ws}/.vscode/settings.json")" == '${userHome}/.config/artagon-vscode/rtk/rtk-init.bash' ]]
  [[ "$(jq -r '.tasks[] | select(.label == "rust: doc") | .command' "${ws}/.vscode/tasks.json")" == "rtk cargo doc --workspace --all-features --no-deps" ]]

  # Case 2: XDG_CONFIG_HOME outside $HOME -> warns about embedding an absolute host path.
  ws2="$(make_workspace rust-rtk-custom-xdg Cargo.toml)"
  XDG_CONFIG_HOME="${TEST_TMP}/external-xdg" \
    run --separate-stderr vspcli apply --target=workspace --check-compat=off --rtk-in-workspace "${ws2}"
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"embeds absolute host path"* ]]
}
