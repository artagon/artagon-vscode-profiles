# ZDOTDIR proxy .zshenv — chains user's original .zshenv safely without recursion.
typeset -g _ARTAGON_PROXY_ZDOTDIR="${ZDOTDIR:-}"
typeset -g _ARTAGON_TARGET_ZDOTDIR="${ARTAGON_ORIG_ZDOTDIR:-${USER_ZDOTDIR:-${HOME}}}"
if [[ -n "${_ARTAGON_TARGET_ZDOTDIR}" && "${_ARTAGON_TARGET_ZDOTDIR}" != "${_ARTAGON_PROXY_ZDOTDIR}" && -r "${_ARTAGON_TARGET_ZDOTDIR}/.zshenv" ]]; then
  source "${_ARTAGON_TARGET_ZDOTDIR}/.zshenv"
fi
