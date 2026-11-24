function __vspcli_profiles
  printf '%s\n' \
    ai \
    ai-copilot \
    ai-plus \
    astro \
    cpp-clangd \
    cpp-intellisense \
    general \
    general-minimal \
    github-workflows \
    java-gradle \
    java-maven \
    java-spring \
    python \
    rust \
    shell
end

# Unified verbs
complete -c vspcli -n '__fish_use_subcommand' -a 'list build validate detect apply reconcile doctor install import open migrate'

# Flags
complete -c vspcli -s l -l list -d 'List profiles'
complete -c vspcli -l detect -d 'Detect workspace toolchain'
complete -c vspcli -l target -d 'Install target' -a 'workspace profile global'
complete -c vspcli -l with -d 'Include capability or toolchain module'
complete -c vspcli -l without -d 'Exclude capability or toolchain module'
complete -c vspcli -l ux -d 'UX preset' -a 'crisp retina default'
complete -c vspcli -l ux-target -d 'UX target (workspace, user, or profile:<name>)' -a 'workspace user profile:'
complete -c vspcli -l ux-in-workspace -d 'Explicitly write personal UX keys to workspace'
complete -c vspcli -l rtk-in-workspace -d 'Explicitly write RTK terminal profile to workspace'
complete -c vspcli -l font -d 'Override editor.fontFamily'
complete -c vspcli -l font-size -d 'Override editor.fontSize'
complete -c vspcli -l theme -d 'Override workbench.colorTheme'
complete -c vspcli -l icon-theme -d 'Override workbench.iconTheme'
complete -c vspcli -l toolchain -d 'Force toolchain flavor' -a '(__vspcli_profiles)'
complete -c vspcli -l no-detect -d 'Disable detection (requires --toolchain)'
complete -c vspcli -l no-rtk -d 'Skip rtk terminal profile emission'
complete -c vspcli -l no-batch -d 'Install extensions one-by-one instead of batched'
complete -c vspcli -l check-compat -d 'Pre-install compat check' -a 'block warn off'
complete -c vspcli -l dry-run -d 'Print would-be writes; modify nothing'
complete -c vspcli -l migrate-catalog -d 'Run catalog migration'
complete -c vspcli -s o -l open -d 'Open profile/path' -a '(__vspcli_profiles)'
complete -c vspcli -s i -l install -d 'Install extensions' -a '(__vspcli_profiles)'
complete -c vspcli -s E -l install-ext -d 'Install extension' -a '(__vspcli_profiles)'
complete -c vspcli -s c -l compose -d 'Compose profiles' -a '(__vspcli_profiles)'
complete -c vspcli -s x -l export -d 'Export profiles' -a '(__vspcli_profiles)'
complete -c vspcli -s O -l open-profiles -d 'Run open-profiles' -a '(__vspcli_profiles)'
complete -c vspcli -s p -l profile-import -d 'Import profile file' -a '(__vspcli_profiles)'
