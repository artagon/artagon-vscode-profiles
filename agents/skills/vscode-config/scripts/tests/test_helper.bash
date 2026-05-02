#!/usr/bin/env bash
# Common test helpers loaded by every .bats file via `load test_helper`.
# Bats sources this without a shebang invocation, but the line above keeps editors happy.

# Resolve the scripts dir (parent of the tests/ dir).
SCRIPTS_DIR="$(CDPATH= cd -- "$(dirname -- "${BATS_TEST_FILENAME}")/.." && pwd)"
export SCRIPTS_DIR

VALIDATE="${SCRIPTS_DIR}/vscode-jsonc-validate"
AUDIT="${SCRIPTS_DIR}/vscode-extensions-audit"
DIFF="${SCRIPTS_DIR}/vscode-profile-diff"
export VALIDATE AUDIT DIFF

FIXTURES="${SCRIPTS_DIR}/tests/fixtures"
export FIXTURES

# Each test gets its own temp dir so parallel runs don't collide.
setup() {
    TEST_TMP="$(mktemp -d)"
    export TEST_TMP
}

teardown() {
    if [ -n "${TEST_TMP:-}" ] && [ -d "$TEST_TMP" ]; then
        rm -rf "$TEST_TMP"
    fi
}

# Helpers
write_file() {
    # write_file <path> <content>          — inline content as 2nd arg
    # write_file <path> <<EOF ... EOF      — stdin (here-doc)
    local path="$1"
    mkdir -p "$(dirname "$path")"
    if [ "$#" -ge 2 ]; then
        printf '%s' "$2" > "$path"
    else
        cat > "$path"
    fi
}

assert_exit() {
    # assert_exit <expected> — verify $status from `run` matches.
    local expected="$1"
    if [ "$status" -ne "$expected" ]; then
        echo "expected exit $expected, got $status"
        echo "--- output ---"
        echo "$output"
        echo "--------------"
        return 1
    fi
}

assert_contains() {
    # assert_contains <substring> — verify $output contains the substring (literal match).
    # Uses grep -F so brackets / globs / regex metacharacters are treated literally.
    local needle="$1"
    if ! printf '%s' "$output" | grep -qF -- "$needle"; then
        echo "expected output to contain: $needle"
        echo "--- output ---"
        echo "$output"
        echo "--------------"
        return 1
    fi
}

assert_not_contains() {
    # Literal-match counterpart to assert_contains.
    local needle="$1"
    if printf '%s' "$output" | grep -qF -- "$needle"; then
        echo "expected output NOT to contain: $needle"
        echo "--- output ---"
        echo "$output"
        echo "--------------"
        return 1
    fi
}
