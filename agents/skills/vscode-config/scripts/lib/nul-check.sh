#!/bin/sh
#
# nul-check.sh — reject files containing raw NUL bytes.
#
# This guard exists because POSIX shell command substitution (`$(...)`) silently
# truncates at the first NUL byte. The three vscode-config validators capture the
# stripped JSONC into a shell variable before handing it to jq, so a file with
# embedded NULs would parse "valid" via the shell pipeline while VS Code (or any
# downstream caller reading the file directly) sees the full content. That is a
# silent validation/audit-bypass surface, especially for vscode-extensions-audit
# (a malicious extensions.json could hide additional `recommendations` after a
# NUL) and vscode-profile-diff (NULs would mask diff content).
#
# Earlier attempts put this guard inside jsonc-strip.awk. That approach failed
# on mawk and BSD awk (default on macOS and Debian) because both implementations
# treat NUL as a C-string terminator inside `length()` and `substr()`: the
# per-line scan never reached the NUL, so the guard was a no-op precisely on
# the platforms most likely to host the validators.
#
# A grep-based approach was also tried but ran into shell command-substitution
# truncation when constructing the search pattern (`$(printf '\0')` produces an
# empty string in POSIX shells). The robust portable check is to count bytes:
# `tr -d '\0'` strips NULs, and `wc -c` counts bytes; if the stripped count is
# smaller than the original count, the file contained at least one NUL. `tr`
# and `wc -c` operate at the byte layer (with LC_ALL=C) and are unaffected by
# C-string terminators.
#
# Usage (sourced):
#   . "$SCRIPT_DIR/lib/nul-check.sh"
#   nul_check_file "$FILE" || exit $?
#
# Exit codes:
#   0 — file does not contain a raw NUL byte
#   1 — file contains a raw NUL byte; an error has been printed to stderr
#   2 — usage error (missing argument or unreadable file)

# shellcheck shell=sh

nul_check_file() {
    nul_check_path="${1-}"

    if [ -z "$nul_check_path" ]; then
        echo "ERROR: nul_check_file: missing path argument" >&2
        return 2
    fi

    if [ ! -r "$nul_check_path" ]; then
        echo "ERROR: nul_check_file: not readable: $nul_check_path" >&2
        return 2
    fi

    # Count raw bytes vs. bytes-with-NULs-stripped. If they differ, the file
    # contains at least one NUL. LC_ALL=C forces byte semantics for `tr`. Both
    # `tr` and `wc -c` are byte-oriented and unaffected by C-string truncation.
    nul_check_total=$(LC_ALL=C wc -c < "$nul_check_path" | tr -d ' ')
    nul_check_stripped=$(LC_ALL=C tr -d '\0' < "$nul_check_path" | LC_ALL=C wc -c | tr -d ' ')

    if [ "$nul_check_total" != "$nul_check_stripped" ]; then
        echo "ERROR: $nul_check_path: input contains raw NUL byte" >&2
        echo "       JSON does not allow raw NULs; encode U+0000 as the six-character escape backslash-u-0-0-0-0." >&2
        return 1
    fi

    return 0
}
