# rtk-init.fish — sourced by VS Code's rtk-fish terminal profile.
# Installs function-based wrappers that route common commands through `rtk`.
# Source-only — do not exec. Idempotent: safe to source twice.
#
# Process-local marker per design.md Decision 22 (NOT exported, so children
# like tmux panes and ssh sessions install their own wrappers if rtk is on
# PATH there).

if not type -q rtk
    echo "rtk not on PATH; aliases not installed" >&2
    exit 0
end

if set -q ARTAGON_RTK_WRAPPED
    exit 0
end
set -l ARTAGON_RTK_WRAPPED 1

function _rtk_wrap --argument-names cmd
    if type -q $cmd
        function $cmd --inherit-variable cmd --wraps=$cmd
            command rtk $cmd $argv
        end
    end
end

# Search & navigation per ~/.agents/SEARCH.md
_rtk_wrap rg
_rtk_wrap grep
_rtk_wrap find

# Build/test wrappers for the toolchains in this repo
for c in git npm pnpm yarn cargo rustc rustup mvn gradle make python python3 pip pip3 node deno bun
    _rtk_wrap $c
end

functions -e _rtk_wrap

# Workspace Trust diagnostic per Decision 14.
if status is-interactive
    echo "rtk wrappers active — workspace trust granted" >&2
end
