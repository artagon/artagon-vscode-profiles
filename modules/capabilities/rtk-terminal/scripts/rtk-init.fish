# rtk-init.fish — sourced by VS Code's rtk-fish terminal profile.
# Installs function-based wrappers that route common commands through `rtk`.
# Source-only — do not exec. Idempotent: safe to source twice.

if not command -q rtk
    echo "rtk not on PATH; aliases not installed" >&2
    return 0
end

if set -q ARTAGON_RTK_WRAPPED
    return 0
end

# Global unexported marker (persists across functions, not inherited by child shells)
set -g ARTAGON_RTK_WRAPPED 1

function _rtk_wrap --argument-names cmd
    if command -q $cmd
        function $cmd --inherit-variable cmd --wraps=$cmd
            command rtk $cmd $argv
        end
    end
end

for c in rg grep find git npm pnpm yarn cargo rustc rustup mvn gradle make python python3 pip pip3 node deno bun
    _rtk_wrap $c
end

functions -e _rtk_wrap

if status is-interactive
    echo "rtk wrappers active — workspace trust granted" >&2
end
