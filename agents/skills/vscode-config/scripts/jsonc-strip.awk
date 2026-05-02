#!/usr/bin/awk -f
#
# jsonc-strip.awk — strip JSONC comments and trailing commas from stdin, emit JSON on stdout.
#
# Handles:
#   - // line comments (outside strings)
#   - /* ... */ block comments (outside strings, multi-line)
#   - Trailing commas before } and ]
#   - String escapes (\" doesn't end a string)
#
# Does NOT handle:
#   - Duplicate keys (jq itself silently keeps the last; vscode-jsonc-validate flags these separately)
#   - JSON5 unquoted keys, single-quoted strings, etc. (VS Code doesn't accept those either)
#
# Usage: awk -f jsonc-strip.awk < input.jsonc > output.json
#
# State machine: char-by-char, track whether we're in a string, in a // comment,
# or in a /* */ comment. Buffer output and post-process trailing commas at the end.

BEGIN {
    in_string = 0
    in_line_comment = 0
    in_block_comment = 0
    escape_next = 0
    output = ""
    saw_first = 0
    BOM = sprintf("%c%c%c", 239, 187, 191)   # UTF-8 BOM: EF BB BF
}

{
    line = $0
    # NUL-byte rejection is enforced by the shell wrappers BEFORE awk is
    # invoked (see scripts/lib/nul-check.sh). Doing it here was unreliable:
    # mawk and BSD awk truncate strings at the C-string NUL inside both
    # length() and substr(), so a per-line scan in awk could not see the
    # NUL and the guard never fired on the platforms where it matters most
    # (macOS, Debian default awk).

    # Strip CR from CRLF line endings — VS Code writes LF on all platforms
    # but real-world files (especially from Windows tools or git on Windows)
    # may arrive with CRLF. The CR is not significant in JSON.
    sub(/\r$/, "", line)

    # Strip UTF-8 BOM (EF BB BF) if present at the very start of the input.
    # Notepad and a few other Windows editors prepend it, and jq rejects BOM-prefixed JSON.
    if (!saw_first) {
        saw_first = 1
        if (substr(line, 1, 3) == BOM) {
            line = substr(line, 4)
        }
    }

    if (NR > 1) line = "\n" line   # preserve newlines between input lines

    n = length(line)
    for (i = 1; i <= n; i++) {
        c = substr(line, i, 1)
        nxt = (i < n) ? substr(line, i+1, 1) : ""

        if (in_line_comment) {
            if (c == "\n") {
                in_line_comment = 0
                output = output c
            }
            continue
        }

        if (in_block_comment) {
            if (c == "*" && nxt == "/") {
                in_block_comment = 0
                i++   # consume the /
            }
            continue
        }

        if (in_string) {
            output = output c
            if (escape_next) {
                escape_next = 0
            } else if (c == "\\") {
                escape_next = 1
            } else if (c == "\"") {
                in_string = 0
            }
            continue
        }

        # Not in any string or comment.
        if (c == "\"") {
            in_string = 1
            output = output c
            continue
        }
        if (c == "/" && nxt == "/") {
            in_line_comment = 1
            i++
            continue
        }
        if (c == "/" && nxt == "*") {
            in_block_comment = 1
            i++
            continue
        }
        output = output c
    }
}

END {
    # Strip trailing commas: a comma followed only by whitespace then } or ].
    # awk's gsub doesn't support backreferences, so do it character-by-character
    # over the buffered output: when we see a comma outside a string, peek ahead
    # over whitespace; if the next non-whitespace char is } or ], drop the comma.
    result = ""
    n = length(output)
    in_str = 0
    esc = 0
    i = 1
    while (i <= n) {
        c = substr(output, i, 1)
        if (in_str) {
            result = result c
            if (esc) {
                esc = 0
            } else if (c == "\\") {
                esc = 1
            } else if (c == "\"") {
                in_str = 0
            }
            i++
            continue
        }
        if (c == "\"") {
            in_str = 1
            result = result c
            i++
            continue
        }
        if (c == ",") {
            # Look ahead over whitespace
            j = i + 1
            while (j <= n) {
                d = substr(output, j, 1)
                if (d == " " || d == "\t" || d == "\n" || d == "\r") {
                    j++
                    continue
                }
                break
            }
            if (j <= n && (substr(output, j, 1) == "}" || substr(output, j, 1) == "]")) {
                # Drop the comma; emit the whitespace and then continue.
                result = result substr(output, i + 1, j - i - 1)
                i = j
                continue
            }
        }
        result = result c
        i++
    }
    printf "%s", result
}
