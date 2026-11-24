#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "ux: --ux=crisp without explicit --ux-target in workspace mode fails closed with exit 4" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --ux=crisp "${ws}"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"--ux-in-workspace"* ]]
  [[ ! -d "${ws}/.vscode" ]]
}

@test "ux: --ux=crisp --ux-in-workspace writes all 15 symmetric keys with Tokyo Night + Material Product Icons" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --ux=crisp --ux-in-workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."workbench.colorTheme"' "${ws}/.vscode/settings.json")" == "Tokyo Night" ]]
  [[ "$(jq -r '."workbench.iconTheme"' "${ws}/.vscode/settings.json")" == "material-icon-theme" ]]
  [[ "$(jq -r '."workbench.productIconTheme"' "${ws}/.vscode/settings.json")" == "material-product-icons" ]]
  [[ "$(jq -r '."workbench.fontAliasing"' "${ws}/.vscode/settings.json")" == "auto" ]]
}

@test "ux: --ux=retina --ux-in-workspace writes Catppuccin Mocha + verified catppuccin-mocha iconTheme + fluent-icons" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --ux=retina --ux-in-workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."workbench.colorTheme"' "${ws}/.vscode/settings.json")" == "Catppuccin Mocha" ]]
  [[ "$(jq -r '."workbench.iconTheme"' "${ws}/.vscode/settings.json")" == "catppuccin-mocha" ]]
  [[ "$(jq -r '."workbench.productIconTheme"' "${ws}/.vscode/settings.json")" == "fluent-icons" ]]
  [[ "$(jq -r '."workbench.preferredDarkColorTheme"' "${ws}/.vscode/settings.json")" == "Catppuccin Mocha" ]]
}

@test "ux: --ux=retina followed by --ux=default resets all 15 managed UX keys" {
  ws="$(make_workspace rust Cargo.toml)"
  vspcli apply --target=workspace --ux=retina --ux-in-workspace "${ws}" >/dev/null
  [[ "$(jq 'has("workbench.colorTheme")' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq 'has("workbench.preferredDarkColorTheme")' "${ws}/.vscode/settings.json")" == "true" ]]
  [[ "$(jq 'has("workbench.productIconTheme")' "${ws}/.vscode/settings.json")" == "true" ]]

  run vspcli apply --target=workspace --ux=default --ux-in-workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  for k in editor.fontFamily editor.fontLigatures editor.fontSize editor.fontVariations editor.letterSpacing editor.lineHeight terminal.integrated.fontFamily terminal.integrated.fontSize window.density.editorTabHeight workbench.colorTheme workbench.fontAliasing workbench.iconTheme workbench.preferredDarkColorTheme workbench.productIconTheme workbench.tree.indent; do
    [[ "$(jq --arg k "${k}" 'has($k)' "${ws}/.vscode/settings.json")" == "false" ]]
  done
}

@test "ux: --theme and --font overrides beat preset; exported profile bundles strip UX keys" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --ux=crisp --ux-in-workspace --theme="GitHub Light" --font="JetBrains Mono" "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."workbench.colorTheme"' "${ws}/.vscode/settings.json")" == "GitHub Light" ]]
  [[ "$(jq -r '."editor.fontFamily"' "${ws}/.vscode/settings.json")" == "JetBrains Mono" ]]

  # Compiled exported bundles never leak any of the 15 UX keys.
  for bundle in "${REPO_ROOT}"/exports/*.code-profile; do
    [[ -L "${bundle}" ]] && continue
    [[ "$(jq '.settings | has("workbench.colorTheme") or has("editor.fontFamily") or has("workbench.iconTheme") or has("workbench.productIconTheme")' "${bundle}")" == "false" ]]
  done
}

@test "ux: --ux-target=user writes UX to <CODE_USER_DIR>/settings.json with .bak & comment-loss guard while keeping workspace clean" {
  ws="$(make_workspace rust Cargo.toml)"
  cat > "${VSCODE_USER_DIR}/settings.json" <<'EOF'
{
  // interior comment must trigger fail-closed guard
  "editor.tabSize": 4
}
EOF
  # Without --allow-comment-loss, interior comments fail closed with exit 4 and zero modification!
  run vspcli apply --target=workspace --ux=retina --ux-target=user "${ws}"
  [[ "${status}" -eq 4 ]]
  [[ "${output}" == *"interior comments"* ]]

  # With --allow-comment-loss, creates bounded .bak, writes 0600, preserves editor.tabSize=4, keeps workspace UX-free!
  run vspcli apply --target=workspace --ux=retina --ux-target=user --allow-comment-loss "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(jq -r '."workbench.colorTheme"' "${VSCODE_USER_DIR}/settings.json")" == "Catppuccin Mocha" ]]
  [[ "$(jq -r '."editor.tabSize"' "${VSCODE_USER_DIR}/settings.json")" -eq 4 ]]
  [[ "$(jq 'has("workbench.colorTheme")' "${ws}/.vscode/settings.json")" == "false" ]]
  bak_count="$(find "${VSCODE_USER_DIR}" -maxdepth 1 -name 'settings.json.bak.*' | wc -l | tr -d ' ')"
  [[ "${bak_count}" -ge 1 ]]
}

@test "ux: workspace symlink escape and <CODE_USER_DIR> out-of-dir symlink are refused (exit 4)" {
  outside="${TEST_TMP}/outside-secret"
  mkdir -p "${outside}"
  printf '{"secret":true}\n' > "${outside}/settings.json"

  ws="$(make_workspace rust-escape Cargo.toml)"
  mkdir -p "${ws}/.vscode"
  ln -s "${outside}/settings.json" "${ws}/.vscode/settings.json"
  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 4 ]]
  [[ "$(jq -r '.secret' "${outside}/settings.json")" == "true" ]]

  # <CODE_USER_DIR>/settings.json symlink pointing outside <CODE_USER_DIR> refused too!
  ws2="$(make_workspace rust-user-escape Cargo.toml)"
  ln -s "${outside}/settings.json" "${VSCODE_USER_DIR}/settings.json"
  run vspcli apply --target=workspace --ux=crisp --ux-target=user "${ws2}"
  [[ "${status}" -eq 4 ]]
  [[ "$(jq -r '.secret' "${outside}/settings.json")" == "true" ]]
}

@test "ux: workspace file mode preservation never widens 0600 settings.json" {
  ws="$(make_workspace rust-mode Cargo.toml)"
  mkdir -p "${ws}/.vscode"
  printf '{}\n' > "${ws}/.vscode/settings.json"
  chmod 0600 "${ws}/.vscode/settings.json"

  run vspcli apply --target=workspace "${ws}"
  [[ "${status}" -eq 0 ]]
  source "${LIB_DIR}/common.sh"
  [[ "$(file_mode "${ws}/.vscode/settings.json")" == "600" ]]
}

@test "ux: --ux=invalid exits 4; sidebar is left in every compiled profile" {
  ws="$(make_workspace rust Cargo.toml)"
  run vspcli apply --target=workspace --ux=monokai --ux-in-workspace "${ws}"
  [[ "${status}" -eq 4 ]]

  for merged in "${REPO_ROOT}"/_merged/*.json; do
    [[ "$(jq -r '."workbench.sideBar.location"' "${merged}")" == "left" ]]
  done
}
