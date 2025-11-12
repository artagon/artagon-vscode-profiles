function __vspcli_profiles
  cat <<'EOF'
ai-profile-crisp
ai-profile-retina
cpp-clangd
cpp-intellisense
java-gradle-crisp
java-gradle-retina
java-maven-crisp
java-maven-retina
java-profile-crisp
java-profile-retina
java-spring-crisp
java-spring-retina
rust-profile-crisp
rust-profile-retina
EOF
end

complete -c vspcli -s l -l list -d 'List profiles'
complete -c vspcli -s o -l open -d 'Open profile/path' -a '(__vspcli_profiles)'
complete -c vspcli -s i -l install -d 'Install extensions' -a '(__vspcli_profiles)'
complete -c vspcli -s E -l install-ext -d 'Install extension' -a '(__vspcli_profiles)'
complete -c vspcli -s c -l compose -d 'Compose profiles' -a '(__vspcli_profiles)'
complete -c vspcli -s x -l export -d 'Export profiles' -a '(__vspcli_profiles)'
complete -c vspcli -s O -l open-profiles -d 'Run open-profiles' -a '(__vspcli_profiles)'
complete -c vspcli -s p -l profile-import -d 'Import profile file' -a '(__vspcli_profiles)'
