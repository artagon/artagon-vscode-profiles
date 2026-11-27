# rtk-init.zsh — sourced by VS Code's rtk-zsh ZDOTDIR proxy.
# Installs function wrappers routing common commands through `rtk`.
# Source-only. Idempotent. Native Zsh.

if [[ -n "${ARTAGON_RTK_WRAPPED:-}" ]]; then
  return 0
fi

if ! (( $+commands[rtk] )); then
  print -u2 -- "rtk not on PATH; aliases not installed"
  return 0
fi

typeset -g ARTAGON_RTK_WRAPPED=1

_rtk_wrap() {
  local cmd="$1"
  (( $+commands[$cmd] )) || return 0
  unalias "$cmd" 2>/dev/null || true
  eval "function ${cmd} { command rtk ${cmd} \"\$@\"; }"
}

local _rtk_cmd
for _rtk_cmd in rg grep find git npm pnpm yarn cargo rustc rustup mvn gradle make python python3 pip pip3 node deno bun; do
  _rtk_wrap "$_rtk_cmd"
done
unfunction _rtk_wrap 2>/dev/null || true
unset _rtk_cmd

if [[ -o interactive ]]; then
  print -u2 -- "rtk wrappers active — workspace trust granted"
fi
