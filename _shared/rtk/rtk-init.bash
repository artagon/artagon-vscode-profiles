# rtk-init.bash — sourced by VS Code's rtk-bash terminal profile.
# Installs function wrappers that route common commands through `rtk`.
# Source-only. Idempotent. Process-local marker per design.md Decision 22.
#
# shellcheck shell=bash

[ -n "${ARTAGON_RTK_WRAPPED:-}" ] && return 0
ARTAGON_RTK_WRAPPED=1  # NOT exported

if ! command -v rtk >/dev/null 2>&1; then
    echo "rtk not on PATH; aliases not installed" >&2
    return 0
fi

_rtk_wrap() {
    local cmd="$1"
    command -v "$cmd" >/dev/null 2>&1 || return 0
    eval "$cmd() { command rtk $cmd \"\$@\"; }"
}

for c in rg grep find git npm pnpm yarn cargo rustc rustup mvn gradle make python python3 pip pip3 node deno bun; do
    _rtk_wrap "$c"
done
unset -f _rtk_wrap

# Workspace Trust diagnostic per Decision 14.
case $- in *i*) echo "rtk wrappers active — workspace trust granted" >&2 ;; esac
