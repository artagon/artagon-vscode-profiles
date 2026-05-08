#compdef vspcli

_vspcli_profiles(){
  local -a profiles
  profiles=(ai ai-plus astro cpp-clangd cpp-intellisense general github-workflows java-gradle java-maven java-spring rust)
  _describe 'profile' profiles
}

_vspcli_targets(){
  local -a targets
  targets=(workspace profile global)
  _describe 'target' targets
}

_vspcli_ux_presets(){
  local -a presets
  presets=(crisp retina default)
  _describe 'ux preset' presets
}

_vspcli_compat(){
  local -a modes
  modes=(block warn off)
  _describe 'compat mode' modes
}

_arguments \
  '(-h --help)'{-h,--help}'[Show help]' \
  '(-l --list)'{-l,--list}'[List profiles]' \
  '--detect[Detect workspace toolchain]:path:_files' \
  '--target[Install target]:target:_vspcli_targets' \
  '--ux[UX preset]:preset:_vspcli_ux_presets' \
  '--font[Override editor.fontFamily]:family:' \
  '--font-size[Override editor.fontSize]:size:' \
  '--theme[Override workbench.colorTheme]:id:' \
  '--icon-theme[Override workbench.iconTheme]:id:' \
  '--toolchain[Force toolchain flavor]:flavor:_vspcli_profiles' \
  '--no-detect[Disable detection]' \
  '--no-rtk[Skip rtk terminal profile emission]' \
  '--check-compat[Pre-install compat check]:mode:_vspcli_compat' \
  '--dry-run[Print would-be writes; modify nothing]' \
  '--migrate-catalog[Run catalog migration]' \
  '(-o --open)'{-o,--open}'[Open profile + path]:profile:_vspcli_profiles:path:_files' \
  '(-i --install)'{-i,--install}'[Install extensions]:profile:_vspcli_profiles' \
  '(-E --install-ext)'{-E,--install-ext}'[Install extension]:profile:_vspcli_profiles:ext:_files' \
  '(-c --compose)'{-c,--compose}'[Compose profiles]:*:profiles:_vspcli_profiles' \
  '(-x --export)'{-x,--export}'[Export profiles]:*:profiles:_vspcli_profiles' \
  '(-O --open-profiles)'{-O,--open-profiles}'[Run open-profiles.sh]:*:profiles:_vspcli_profiles' \
  '(-p --profile-import)'{-p,--profile-import}'[Import .code-profile]:profile:_vspcli_profiles:file:_files'
