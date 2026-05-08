function __vspcli_profiles
  cat <<'EOF'
ai
ai-plus
astro
cpp-clangd
cpp-intellisense
general
github-workflows
java-gradle
java-maven
java-spring
rust
EOF
end

# Modern flags
complete -c vspcli -s l -l list -d 'List profiles'
complete -c vspcli -l detect -d 'Detect workspace toolchain'
complete -c vspcli -l target -d 'Install target' -a 'workspace profile global'
complete -c vspcli -l ux -d 'UX preset' -a 'crisp retina default'
complete -c vspcli -l font -d 'Override editor.fontFamily'
complete -c vspcli -l font-size -d 'Override editor.fontSize'
complete -c vspcli -l theme -d 'Override workbench.colorTheme'
complete -c vspcli -l icon-theme -d 'Override workbench.iconTheme'
complete -c vspcli -l toolchain -d 'Force toolchain flavor' -a '(__vspcli_profiles)'
complete -c vspcli -l no-detect -d 'Disable detection (requires --toolchain)'
complete -c vspcli -l no-rtk -d 'Skip rtk terminal profile emission'
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
