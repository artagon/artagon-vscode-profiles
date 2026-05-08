#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "ux: --ux=crisp writes Tokyo Night theme" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --ux=crisp "$ws"
  [ "$status" -eq 0 ]
  theme=$(jq -r '."workbench.colorTheme"' "$ws/.vscode/settings.json")
  [ "$theme" = "Tokyo Night" ]
}

@test "ux: --ux=retina writes Catppuccin Mocha theme" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --ux=retina "$ws"
  [ "$status" -eq 0 ]
  theme=$(jq -r '."workbench.colorTheme"' "$ws/.vscode/settings.json")
  [ "$theme" = "Catppuccin Mocha" ]
}

@test "ux: --theme override beats preset" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --ux=crisp --theme="GitHub Light" "$ws"
  [ "$status" -eq 0 ]
  theme=$(jq -r '."workbench.colorTheme"' "$ws/.vscode/settings.json")
  [ "$theme" = "GitHub Light" ]
}

@test "ux: --font sets editor.fontFamily" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --font="JetBrains Mono" "$ws"
  [ "$status" -eq 0 ]
  font=$(jq -r '."editor.fontFamily"' "$ws/.vscode/settings.json")
  [ "$font" = "JetBrains Mono" ]
}

@test "ux: editor-base keys land alongside UX preset" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --ux=crisp "$ws"
  [ "$status" -eq 0 ]
  # editor-base key (telemetry-off) should be present
  telemetry=$(jq -r '."telemetry.telemetryLevel"' "$ws/.vscode/settings.json")
  [ "$telemetry" = "off" ]
}

@test "ux: --ux=default applies no UX tweaks" {
  require_bin jq
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --ux=default "$ws"
  [ "$status" -eq 0 ]
  # No UX keys overlaid: workbench.colorTheme should NOT be present in
  # settings.json (since editor-base doesn't set it and default.jsonc is empty).
  has_theme=$(jq 'has("workbench.colorTheme")' "$ws/.vscode/settings.json")
  [ "$has_theme" = "false" ]
  has_icon=$(jq 'has("workbench.iconTheme")' "$ws/.vscode/settings.json")
  [ "$has_icon" = "false" ]
}

@test "ux: --ux=invalid exits 4 with valid list" {
  ws=$(make_workspace rust Cargo.toml)
  run vspcli --detect --target=workspace --ux=monokai "$ws"
  [ "$status" -eq 4 ]
}

@test "ux: --ux preset and editor-base have disjoint keys" {
  require_bin jq
  ux_keys=$(awk -f "$LIB_DIR/jsonc-strip.awk" "$REPO_ROOT/_shared/ux/crisp.jsonc" | jq -S -c 'keys')
  base_keys=$(awk -f "$LIB_DIR/jsonc-strip.awk" "$REPO_ROOT/_shared/editor-base.jsonc" | jq -S -c 'keys')
  overlap=$(jq -n --argjson a "$ux_keys" --argjson b "$base_keys" '$a - ($a - $b)')
  [ "$overlap" = "[]" ]
}
