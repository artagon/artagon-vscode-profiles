# ZDOTDIR proxy .zprofile — chains user's original .zprofile for login shells.
typeset -g _ARTAGON_PROXY_ZDOTDIR="${_ARTAGON_PROXY_ZDOTDIR:-${ZDOTDIR:-}}"
typeset -g _ARTAGON_TARGET_ZDOTDIR="${ARTAGON_ORIG_ZDOTDIR:-${USER_ZDOTDIR:-${HOME}}}"
if [[ -n "${_ARTAGON_TARGET_ZDOTDIR}" && "${_ARTAGON_TARGET_ZDOTDIR}" != "${_ARTAGON_PROXY_ZDOTDIR}" && -r "${_ARTAGON_TARGET_ZDOTDIR}/.zprofile" ]]; then
  source "${_ARTAGON_TARGET_ZDOTDIR}/.zprofile"
fi
