# ZDOTDIR proxy .zlogin — chains user's .zlogin and restores original ZDOTDIR state.
typeset -g _ARTAGON_PROXY_ZDOTDIR="${_ARTAGON_PROXY_ZDOTDIR:-${ZDOTDIR:-}}"
typeset -g _ARTAGON_TARGET_ZDOTDIR="${ARTAGON_ORIG_ZDOTDIR:-${USER_ZDOTDIR:-${HOME}}}"
if [[ -n "${_ARTAGON_TARGET_ZDOTDIR}" && "${_ARTAGON_TARGET_ZDOTDIR}" != "${_ARTAGON_PROXY_ZDOTDIR}" && -r "${_ARTAGON_TARGET_ZDOTDIR}/.zlogin" ]]; then
  source "${_ARTAGON_TARGET_ZDOTDIR}/.zlogin"
fi
if [[ "${ARTAGON_ORIG_ZDOTDIR_SET:-0}" == "1" ]]; then
  export ZDOTDIR="${ARTAGON_ORIG_ZDOTDIR}"
else
  unset ZDOTDIR
fi
