#!/usr/bin/env bats
#
# Tests for vscode-profile-diff.

load test_helper

@test "identical files exit 0" {
    write_file "$TEST_TMP/a.json" '{"a": 1, "b": 2}'
    write_file "$TEST_TMP/b.json" '{"a": 1, "b": 2}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 0
    assert_contains "No differences"
}

@test "different keys reported as added/removed" {
    write_file "$TEST_TMP/a.json" '{"only_a": 1}'
    write_file "$TEST_TMP/b.json" '{"only_b": 2}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    assert_contains "only_a"
    assert_contains "only_b"
    assert_contains "would be removed"
    assert_contains "would be added"
}

@test "changed values are reported with both sides" {
    write_file "$TEST_TMP/a.json" '{"editor.tabSize": 2}'
    write_file "$TEST_TMP/b.json" '{"editor.tabSize": 4}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    assert_contains "Changed values"
    assert_contains "editor.tabSize"
    assert_contains "left:  2"
    assert_contains "right: 4"
}

@test "key order does not matter" {
    write_file "$TEST_TMP/a.json" '{"a": 1, "b": 2, "c": 3}'
    write_file "$TEST_TMP/b.json" '{"c": 3, "a": 1, "b": 2}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 0
}

@test "JSONC comments don't affect diff" {
    write_file "$TEST_TMP/a.json" <<'EOF'
{
  // useful settings
  "a": 1,
  "b": 2
}
EOF
    write_file "$TEST_TMP/b.json" <<'EOF'
{
  /* documentation differs */
  "a": 1,
  "b": 2
}
EOF
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 0
}

@test "trailing commas don't affect diff" {
    write_file "$TEST_TMP/a.json" '{"a": 1, "b": 2}'
    write_file "$TEST_TMP/b.json" '{"a": 1, "b": 2,}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 0
}

@test "--json output is parseable JSON" {
    write_file "$TEST_TMP/a.json" '{"a": 1, "b": 2}'
    write_file "$TEST_TMP/b.json" '{"a": 1, "c": 3}'
    run "$DIFF" --json "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    # Output should be parseable
    echo "$output" | jq empty
    # And should have the four expected arrays
    [ "$(echo "$output" | jq -r '.only_left[]')" = "b" ]
    [ "$(echo "$output" | jq -r '.only_right[]')" = "c" ]
    [ "$(echo "$output" | jq -r '.identical[]')" = "a" ]
    [ "$(echo "$output" | jq '.changed | length')" = "0" ]
}

@test "--json output for identical files is valid empty diff" {
    write_file "$TEST_TMP/a.json" '{"a": 1}'
    write_file "$TEST_TMP/b.json" '{"a": 1}'
    run "$DIFF" --json "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 0
    echo "$output" | jq empty
    [ "$(echo "$output" | jq '.only_left | length')" = "0" ]
    [ "$(echo "$output" | jq '.only_right | length')" = "0" ]
    [ "$(echo "$output" | jq '.changed | length')" = "0" ]
}

@test "--verbose shows identical keys" {
    write_file "$TEST_TMP/a.json" '{"same": 1, "diff": 2}'
    write_file "$TEST_TMP/b.json" '{"same": 1, "diff": 3}'
    run "$DIFF" --verbose "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    assert_contains "Identical"
    assert_contains "= same"
}

@test "without --verbose, identical keys are not listed" {
    write_file "$TEST_TMP/a.json" '{"same": 1, "diff": 2}'
    write_file "$TEST_TMP/b.json" '{"same": 1, "diff": 3}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    # The "= same" line should not appear (that's the verbose-only marker)
    assert_not_contains "= same"
}

@test "object value differences detected" {
    write_file "$TEST_TMP/a.json" '{"obj": {"x": 1}}'
    write_file "$TEST_TMP/b.json" '{"obj": {"x": 2}}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    assert_contains "obj"
}

@test "language-specific override blocks compared as objects" {
    write_file "$TEST_TMP/a.json" '{"[python]": {"editor.tabSize": 4}}'
    write_file "$TEST_TMP/b.json" '{"[python]": {"editor.tabSize": 2}}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    assert_contains "[python]"
}

@test "missing left file exits 2" {
    write_file "$TEST_TMP/b.json" '{}'
    run "$DIFF" /nonexistent.json "$TEST_TMP/b.json"
    assert_exit 2
}

@test "missing right file exits 2" {
    write_file "$TEST_TMP/a.json" '{}'
    run "$DIFF" "$TEST_TMP/a.json" /nonexistent.json
    assert_exit 2
}

@test "invalid JSONC on left exits 2" {
    write_file "$TEST_TMP/a.json" '{ broken'
    write_file "$TEST_TMP/b.json" '{}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 2
    assert_contains "not valid JSONC"
}

@test "non-object input exits 2" {
    write_file "$TEST_TMP/a.json" '[1, 2, 3]'
    write_file "$TEST_TMP/b.json" '{}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 2
    assert_contains "not a JSON object"
}

@test "summary line counts match output" {
    write_file "$TEST_TMP/a.json" '{"a": 1, "b": 2, "c": 3}'
    write_file "$TEST_TMP/b.json" '{"a": 1, "b": 99, "d": 4}'
    run "$DIFF" "$TEST_TMP/a.json" "$TEST_TMP/b.json"
    assert_exit 1
    # 1 removed (c), 1 added (d), 1 changed (b), 1 identical (a)
    assert_contains "Summary: 1 removed, 1 added, 1 changed, 1 identical"
}

@test "missing both args exits 2 with usage" {
    run "$DIFF"
    assert_exit 2
}

@test "single arg exits 2 with usage" {
    write_file "$TEST_TMP/a.json" '{}'
    run "$DIFF" "$TEST_TMP/a.json"
    assert_exit 2
}

@test "diffing a profile against itself is a no-op" {
    write_file "$TEST_TMP/profile.json" <<'EOF'
{
  "editor.tabSize": 2,
  "editor.fontSize": 14,
  "[python]": { "editor.tabSize": 4 }
}
EOF
    run "$DIFF" "$TEST_TMP/profile.json" "$TEST_TMP/profile.json"
    assert_exit 0
    assert_contains "No differences"
    assert_contains "3 key(s) match"
}

@test "BOM in left file does not affect diff" {
    printf '\357\273\277{"a":1,"b":2}' > "$TEST_TMP/bom.json"
    write_file "$TEST_TMP/no-bom.json" '{"a":1,"b":2}'
    run "$DIFF" "$TEST_TMP/bom.json" "$TEST_TMP/no-bom.json"
    assert_exit 0
}

@test "CRLF line endings do not produce spurious diffs" {
    printf '{\r\n  "a": 1,\r\n  "b": 2\r\n}' > "$TEST_TMP/crlf.json"
    write_file "$TEST_TMP/lf.json" '{"a": 1, "b": 2}'
    run "$DIFF" "$TEST_TMP/crlf.json" "$TEST_TMP/lf.json"
    assert_exit 0
}

# ---------- R1-12: locale-deterministic ----------

@test "R1-12: profile-diff set-ops behave identically under Turkish locale" {
    write_file "$TEST_TMP/left.json" <<'EOF'
{ "INDEX": 1, "index": 2, "editor.tabSize": 4 }
EOF
    write_file "$TEST_TMP/right.json" <<'EOF'
{ "INDEX": 1, "index": 99 }
EOF
    LC_ALL=tr_TR.UTF-8 LANG=tr_TR.UTF-8 run "$DIFF" --json "$TEST_TMP/left.json" "$TEST_TMP/right.json"
    # Should successfully diff (exit 1 = differs, 0 = identical, 2 = error)
    [ "$status" -eq 1 ]
}

# ---------- R1-13: NUL-byte rejection on either side ----------

@test "R1-13: profile-diff rejects NUL byte on left input" {
    printf '{"a":1}\0{"hidden":true}' > "$TEST_TMP/left.json"
    write_file "$TEST_TMP/right.json" '{"a":1}'
    run "$DIFF" "$TEST_TMP/left.json" "$TEST_TMP/right.json"
    [ "$status" -ne 0 ]
    assert_contains "raw NUL byte"
}

@test "R1-13: profile-diff rejects NUL byte on right input" {
    write_file "$TEST_TMP/left.json" '{"a":1}'
    printf '{"a":1}\0{"hidden":true}' > "$TEST_TMP/right.json"
    run "$DIFF" "$TEST_TMP/left.json" "$TEST_TMP/right.json"
    [ "$status" -ne 0 ]
    assert_contains "raw NUL byte"
}
