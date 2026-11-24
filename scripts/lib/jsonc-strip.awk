#!/usr/bin/awk -f
#
# jsonc-strip.awk — strip JSONC comments and trailing commas from stdin, emit JSON on stdout.
# Always invoke under LC_ALL=C for deterministic byte-level UTF-8 handling across BSD/GNU awk.
#
# Hardened invariants:
#   1. Comments (// and /* */) outside strings are replaced with a SINGLE SPACE (never empty
#      string), so malformed tokens like `{"limit":1/*x*/2}` stay invalid `1 2` instead of `12`.
#   2. Unterminated /* block comments fail with exit 1.
#   3. Trailing commas before } or ] are stripped ONLY when preceded by a valid JSON value end
#      token (`"`, `]`, `}`, digit 0-9, `e` of true/false, `l` of null), so malformed `{"list":[,]}`
#      or `{"list":[1,,]}` stays invalid and is rejected by jq.

BEGIN {
    in_string = 0
    in_line_comment = 0
    in_block_comment = 0
    escape_next = 0
    output = ""
    saw_first = 0
    BOM = sprintf("%c%c%c", 239, 187, 191)
}

{
    line = $0
    sub(/\r$/, "", line)

    if (!saw_first) {
        saw_first = 1
        if (substr(line, 1, 3) == BOM) {
            line = substr(line, 4)
        }
    }

    if (NR > 1) line = "\n" line

    n = length(line)
    for (i = 1; i <= n; i++) {
        c = substr(line, i, 1)
        nxt = (i < n) ? substr(line, i + 1, 1) : ""

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
                output = output " "
                i++
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

        if (c == "\"") {
            in_string = 1
            output = output c
            continue
        }
        if (c == "/" && nxt == "/") {
            in_line_comment = 1
            output = output " "
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
    if (in_block_comment) {
        print "ERROR: jsonc-strip.awk: unterminated /* block comment at end of input" > "/dev/stderr"
        exit 1
    }

    result = ""
    n = length(output)
    in_str = 0
    esc = 0
    last_sig = ""
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
                last_sig = "\""
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
            # Peek ahead over whitespace.
            j = i + 1
            while (j <= n) {
                d = substr(output, j, 1)
                if (d == " " || d == "\t" || d == "\n" || d == "\r") {
                    j++
                    continue
                }
                break
            }
            # Only strip comma when preceded by a valid JSON value-end char AND followed by } or ].
            valid_prev = (last_sig == "\"" || last_sig == "}" || last_sig == "]" || last_sig == "e" || last_sig == "l" || (last_sig >= "0" && last_sig <= "9"))
            if (valid_prev && j <= n && (substr(output, j, 1) == "}" || substr(output, j, 1) == "]")) {
                result = result substr(output, i + 1, j - i - 1)
                i = j
                continue
            }
            last_sig = ","
            result = result c
            i++
            continue
        }
        if (c != " " && c != "\t" && c != "\n" && c != "\r") {
            last_sig = c
        }
        result = result c
        i++
    }
    printf "%s", result
}
