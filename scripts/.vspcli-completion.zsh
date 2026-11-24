#compdef vspcli

_vspcli_verbs(){
  local -a verbs
  verbs=(
    'list:List compiled profile recipes'
    'build:Compile settings, extensions, bundles, symlinks, and SHA256SUMS'
    'validate:Run all catalog, module, DAG, scope, and security checks'
    'detect:Detect workspace toolchains and optionally apply config'
    'apply:Apply modules/UX settings to workspace, profile, or global target'
    'reconcile:Audit and sync extensions for a VS Code profile'
    'doctor:Check host prerequisites, Default profile scopes, and compromised extensions'
    'install:Install extensions for a profile'
    'import:Import a .code-profile bundle'
    'open:Open VS Code with a profile'
    'migrate:Run catalog migration diagnostics or symlink refresh'
  )
  _describe 'verb' verbs
}

_vspcli_profiles(){
  local -a profiles
  profiles=(ai ai-copilot ai-plus astro cpp-clangd cpp-intellisense general general-minimal github-workflows java-gradle java-maven java-spring python rust shell)
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
  '1:verb:_vspcli_verbs' \
  '(-h --help)'{-h,--help}'[Show help]' \
  '(-l --list)'{-l,--list}'[List profiles]' \
  '--detect[Detect workspace toolchain]:path:_files' \
  '--target[Install target]:target:_vspcli_targets' \
  '--with[Add capability or toolchain module]:module:' \
  '--without[Exclude capability or toolchain module]:module:' \
  '--ux[UX preset]:preset:_vspcli_ux_presets' \
  '--ux-target[UX target (workspace, user, or profile:<name>)]:target:(workspace user profile\:)' \
  '--ux-in-workspace[Explicitly write personal UX keys to workspace]' \
  '--rtk-in-workspace[Explicitly write RTK terminal profile to workspace]' \
  '--font[Override editor.fontFamily]:family:' \
  '--font-size[Override editor.fontSize]:size:' \
  '--theme[Override workbench.colorTheme]:id:' \
  '--icon-theme[Override workbench.iconTheme]:id:' \
  '--toolchain[Force toolchain flavor]:flavor:_vspcli_profiles' \
  '--no-detect[Disable detection]' \
  '--no-rtk[Skip rtk terminal profile emission]' \
  '--no-batch[Install extensions one-by-one instead of batched]' \
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
