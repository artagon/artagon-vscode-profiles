#!/usr/bin/env bats
# bats file_tags=no-parallel

bats_require_minimum_version 1.5.0
load test_helper

RTK_DIR="${REPO_ROOT}/packs/rtk-terminal/scripts"

@test "rtk-init.bash: missing rtk emits clear stderr and leaves ARTAGON_RTK_WRAPPED unset so later PATH retry works" {
  bin="${TEST_TMP}/late-rtk"
  mkdir -p "${bin}"
  printf '#!/bin/sh\necho "rtk-called:$*"\n' > "${bin}/rtk"
  chmod 0755 "${bin}/rtk"

  run --separate-stderr env -i HOME="${HOME}" PATH="/usr/bin:/bin" "${BASH}" -c "
    source '${RTK_DIR}/rtk-init.bash'
    echo \"first:\${ARTAGON_RTK_WRAPPED:-unset}\"
    PATH='${bin}:/usr/bin:/bin'
    source '${RTK_DIR}/rtk-init.bash'
    echo \"second:\${ARTAGON_RTK_WRAPPED:-unset}\"
  "
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"rtk not on PATH"* ]]
  [[ "${output}" == *"first:unset"* ]]
  [[ "${output}" == *"second:1"* ]]
}

@test "rtk-init.bash: _ARTAGON_RTK_RC_LOADED prevents ~/.bashrc recursion and handles Ubuntu alias grep with exit 0" {
  bin="${TEST_TMP}/bash-bin"
  mkdir -p "${bin}"
  printf '#!/bin/sh\necho "rtk-stub:$*"\n' > "${bin}/rtk"
  printf '#!/bin/sh\necho "real-grep"\n' > "${bin}/grep"
  chmod 0755 "${bin}/rtk" "${bin}/grep"

  # Simulate Ubuntu ~/.bashrc defining `alias grep='grep --color=auto'` AND re-sourcing rtk-init.bash!
  cat > "${HOME}/.bashrc" <<EOF
alias grep='grep --color=auto'
source '${RTK_DIR}/rtk-init.bash'
EOF

  run env -i HOME="${HOME}" PATH="${bin}:/usr/bin:/bin" "${BASH}" -c "
    source '${RTK_DIR}/rtk-init.bash'
    rc=\$?
    echo \"rc=\${rc}\"
    type -t grep
    grep foo
  "
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"rc=0"* ]]
  [[ "${output}" == *"function"* ]]
  [[ "${output}" == *"rtk-stub:grep foo"* ]]
}

@test "rtk-init.bash: ARTAGON_RTK_WRAPPED is process-local (never exported to child bash)" {
  bin="${TEST_TMP}/local-bin"
  mkdir -p "${bin}"
  printf '#!/bin/sh\necho rtk-stub\n' > "${bin}/rtk"
  chmod 0755 "${bin}/rtk"

  result="$(env -i HOME="${HOME}" PATH="${bin}:/usr/bin:/bin" "${BASH}" -c "
    source '${RTK_DIR}/rtk-init.bash' 2>/dev/null
    '${BASH}' -c 'echo \${ARTAGON_RTK_WRAPPED:-unset}'
  ")"
  [[ "${result}" = "unset" ]]
}

@test "rtk-init.fish: handles spaces in XDG/script path and retries cleanly after PATH update" {
  require_bin fish
  spaced="${TEST_TMP}/dir with spaces"
  mkdir -p "${spaced}/bin"
  cp "${RTK_DIR}/rtk-init.fish" "${spaced}/rtk-init.fish"
  printf '#!/bin/sh\necho "rtk-fish:$*"\n' > "${spaced}/bin/rtk"
  chmod 0755 "${spaced}/bin/rtk"

  fish_bin="$(command -v fish)"
  sandbox="${TEST_TMP}/fish-only"
  mkdir -p "${sandbox}"
  ln -s "${fish_bin}" "${sandbox}/fish"

  run --separate-stderr env -i HOME="${HOME}" PATH="${sandbox}:/usr/bin:/bin" "${sandbox}/fish" --no-config -c "
    source '${spaced}/rtk-init.fish'
    set -gx PATH '${spaced}/bin' /usr/bin /bin
    source '${spaced}/rtk-init.fish'
    if functions -q find
      echo wrapped-yes
    else
      echo wrapped-no
    end
  "
  [[ "${status}" -eq 0 ]]
  [[ "${stderr}" == *"rtk not on PATH"* ]]
  [[ "${output}" == *"wrapped-yes"* ]]
}

@test "rtk-init.zsh: ZDOTDIR proxy restores unset, HOME, and custom ZDOTDIR in both non-login and login shells" {
  require_bin zsh
  bin="${TEST_TMP}/zsh-bin"
  mkdir -p "${bin}"
  printf '#!/bin/sh\necho "rtk-zsh:$*"\n' > "${bin}/rtk"
  chmod 0755 "${bin}/rtk"

  # Prepare custom ZDOTDIR with user .zshrc and .zlogin markers.
  custom_z="${TEST_TMP}/custom-zdot"
  mkdir -p "${custom_z}"
  printf 'USER_ZSHRC_RAN=1\n' > "${custom_z}/.zshrc"
  printf 'USER_ZLOGIN_RAN=1\n' > "${custom_z}/.zlogin"
  printf 'HOME_ZSHRC_RAN=1\n' > "${HOME}/.zshrc"
  printf 'HOME_ZLOGIN_RAN=1\n' > "${HOME}/.zlogin"

  proxy_dir="${RTK_DIR}/zsh"

  # Case 1: ZDOTDIR originally UNSET -> non-login interactive shell restores unset ZDOTDIR & wraps find.
  run env -i HOME="${HOME}" PATH="${bin}:/usr/bin:/bin" \
    ZDOTDIR="${proxy_dir}" \
    ARTAGON_ORIG_ZDOTDIR_SET=0 \
    ARTAGON_ORIG_ZDOTDIR="" \
    zsh -i -c 'echo "home_rc=${HOME_ZSHRC_RAN:-0} zdot=${ZDOTDIR-__UNSET__} wrapped=${ARTAGON_RTK_WRAPPED:-0}"; whence -w find'
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"home_rc=1 zdot=__UNSET__ wrapped=1"* ]]
  [[ "${output}" == *"find: function"* ]]

  # Case 2: ZDOTDIR originally CUSTOM -> login interactive shell sources both user .zshrc + .zlogin and restores custom ZDOTDIR!
  run env -i HOME="${HOME}" PATH="${bin}:/usr/bin:/bin" \
    ZDOTDIR="${proxy_dir}" \
    ARTAGON_ORIG_ZDOTDIR_SET=1 \
    ARTAGON_ORIG_ZDOTDIR="${custom_z}" \
    zsh -l -i -c 'echo "rc=${USER_ZSHRC_RAN:-0} login=${USER_ZLOGIN_RAN:-0} zdot=${ZDOTDIR} wrapped=${ARTAGON_RTK_WRAPPED:-0}"'
  [[ "${status}" -eq 0 ]]
  [[ "${output}" == *"rc=1 login=1 zdot=${custom_z} wrapped=1"* ]]
}
