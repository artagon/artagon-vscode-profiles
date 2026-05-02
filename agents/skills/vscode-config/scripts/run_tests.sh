#!/usr/bin/env bash
#
# run_tests.sh — execute all bats test suites in tests/.
#
# Usage:
#   ./run_tests.sh              # run everything
#   ./run_tests.sh --tap        # TAP output
#   ./run_tests.sh <pattern>    # only run tests matching the pattern (bats --filter)

set -eu

# shellcheck disable=SC1007
# `CDPATH= cd` is the deliberate "neutralize CDPATH for this one cd" idiom.
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
TESTS_DIR="${SCRIPT_DIR}/tests"

command -v bats >/dev/null 2>&1 || {
    echo "bats not found. Install with:" >&2
    echo "  Ubuntu/Debian: apt-get install bats" >&2
    echo "  macOS:         brew install bats-core" >&2
    echo "  Source:        https://github.com/bats-core/bats-core" >&2
    exit 1
}
command -v jq >/dev/null 2>&1 || {
    echo "jq not found. Install with:" >&2
    echo "  Ubuntu/Debian: apt-get install jq" >&2
    echo "  macOS:         brew install jq" >&2
    exit 1
}

ARGS=()
FILTER=""
for arg in "$@"; do
    case "$arg" in
        --tap) ARGS+=(--tap) ;;
        --*)   ARGS+=("$arg") ;;
        *)     FILTER="$arg" ;;
    esac
done

if [ -n "$FILTER" ]; then
    bats "${ARGS[@]}" --filter "$FILTER" "$TESTS_DIR"
else
    bats "${ARGS[@]}" "$TESTS_DIR"
fi
