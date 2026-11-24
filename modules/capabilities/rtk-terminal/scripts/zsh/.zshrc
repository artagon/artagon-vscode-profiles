# ZDOTDIR proxy .zshrc — chains user's .zshrc, loads rtk-init.zsh, and restores ZDOTDIR
# immediately for non-login shells (login shells defer ZDOTDIR restore to .zlogin).
typeset -g _ARTAGON_PROXY_ZDOTDIR="${_ARTAGON_PROXY_ZDOTDIR:-${ZDOTDIR:-}}"
typeset -g _ARTAGON_TARGET_ZDOTDIR="${ARTAGON_ORIG_ZDOTDIR:-${USER_ZDOTDIR:-${HOME}}}"
if [[ -n "${_ARTAGON_TARGET_ZDOTDIR}" && "${_ARTAGON_TARGET_ZDOTDIR}" != "${_ARTAGON_PROXY_ZDOTDIR}" && -r "${_ARTAGON_TARGET_ZDOTDIR}/.zshrc" ]]; then
  source "${_ARTAGON_TARGET_ZDOTDIR}/.zshrc"
fi
if [[ -n "${_ARTAGON_PROXY_ZDOTDIR}" && -r "${_ARTAGON_PROXY_ZDOTDIR}/../rtk-init.zsh" ]]; then
  source "${_ARTAGON_PROXY_ZDOTDIR}/../rtk-init.zsh"
fi
if [[ ! -o login ]]; then
  if [[ "${ARTAGON_ORIG_ZDOTDIR_SET:-0}" == "1" ]]; then
    export ZDOTDIR="${ARTAGON_ORIG_ZDOTDIR}"
  else
    unset ZDOTDIR
  fi
fi
