_vspcli_profiles(){
  cat <<'EOF
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
}
#compdef vspcli
_arguments   '(-h --help)'{-h,--help}'[Show help]'   '(-l --list)'{-l,--list}'[List profiles]'   '(-o --open)'{-o,--open}'[Open profile + path]:profile:__vspcli_profiles:path:_files'   '(-i --install)'{-i,--install}'[Install extensions]:profile:__vspcli_profiles'   '(-E --install-ext)'{-E,--install-ext}'[Install extension]:profile:__vspcli_profiles:ext:_files'   '(-c --compose)'{-c,--compose}'[Compose profiles]:*:profiles:__vspcli_profiles'   '(-x --export)'{-x,--export}'[Export profiles]:*:profiles:__vspcli_profiles'   '(-O --open-profiles)'{-O,--open-profiles}'[Run open-profiles.sh]:*:profiles:__vspcli_profiles'   '(-p --profile-import)'{-p,--profile-import}'[Import .code-profile]:profile:__vspcli_profiles:file:_files'

