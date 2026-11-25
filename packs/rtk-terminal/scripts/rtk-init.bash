# rtk-init.bash — sourced by VS Code's rtk-bash terminal profile.
# Installs function wrappers that route common commands through `rtk`.
# Source-only. Idempotent. Bash 3.2+ compatible.
#
# shellcheck shell=bash

if [[ -z "${_ARTAGON_RTK_RC_LOADED:-}" ]]; then
  _ARTAGON_RTK_RC_LOADED=1
  if shopt -q login_shell 2>/dev/null; then
    if [[ -r "${HOME}/.bash_profile" ]]; then
      # shellcheck disable=SC1091
      source "${HOME}/.bash_profile"
    elif [[ -r "${HOME}/.profile" ]]; then
      # shellcheck disable=SC1091
      source "${HOME}/.profile"
    fi
  elif [[ -r "${HOME}/.bashrc" ]]; then
    # shellcheck disable=SC1091
    source "${HOME}/.bashrc"
  fi
fi

if [[ -n "${ARTAGON_RTK_WRAPPED:-}" ]]; then
  return 0
fi

if ! command -v rtk >/dev/null 2>&1; then
  echo "rtk not on PATH; aliases not installed" >&2
  return 0
fi

ARTAGON_RTK_WRAPPED=1 # NOT exported (process-local)

_rtk_wrap() {
  local cmd="${1}"
  type -P "${cmd}" >/dev/null 2>&1 || return 0
  unalias "${cmd}" 2>/dev/null || true
  eval "function ${cmd} { command rtk ${cmd} \"\${@+\"\$@\"}\"; }"
}

for _rtk_cmd in rg grep find git npm pnpm yarn cargo rustc rustup mvn gradle make python python3 pip pip3 node deno bun; do
  _rtk_wrap "${_rtk_cmd}"
done
unset -f _rtk_wrap
unset _rtk_cmd

if [[ $- == *i* ]]; then
  echo "rtk wrappers active — workspace trust granted" >&2
fi
