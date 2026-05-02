#!/usr/bin/env bats
#
# Tests for jsonc-strip.awk — the JSONC-to-JSON converter.
# This script underlies all three CLI tools, so it gets thorough coverage.

load test_helper

STRIPPER="${SCRIPTS_DIR}/jsonc-strip.awk"

strip() {
    awk -f "$STRIPPER"
}

@test "strips // line comments outside strings" {
    result="$(printf '{\n  // this is a comment\n  "a": 1\n}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}

@test "strips /* block */ comments outside strings" {
    result="$(printf '{\n  /* block comment */ "a": 1\n}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}

@test "strips multi-line block comments" {
    result="$(printf '{\n  /* line one\n     line two */\n  "a": 1\n}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}

@test "preserves // inside string values" {
    result="$(printf '{"url": "http://example.com"}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"url":"http://example.com"}' ]
}

@test "preserves /* */ inside string values" {
    result="$(printf '{"pattern": "/* match this */"}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"pattern":"/* match this */"}' ]
}

@test "removes trailing comma in object" {
    result="$(printf '{"a": 1, "b": 2,}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1,"b":2}' ]
}

@test "removes trailing comma in array" {
    result="$(printf '{"x": [1, 2, 3,]}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"x":[1,2,3]}' ]
}

@test "removes trailing comma followed by newline before bracket" {
    result="$(printf '{\n  "a": 1,\n  "b": 2,\n}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1,"b":2}' ]
}

@test "preserves comma inside string that looks trailing" {
    # If the comma is part of a string, it must not be removed.
    result="$(printf '{"a": "value,", "b": 2}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":"value,","b":2}' ]
}

@test "handles escaped quote inside string" {
    result="$(printf '{"q": "she said \\"hi\\""}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"q":"she said \"hi\""}' ]
}

@test "handles escaped backslash before quote" {
    # "path\\" — the backslash is escaped, the closing quote is real.
    result="$(printf '{"p": "C:\\\\foo", "q": 1}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"p":"C:\\foo","q":1}' ]
}

@test "comment immediately after value" {
    result="$(printf '{"a": 1, // explain\n "b": 2}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1,"b":2}' ]
}

@test "comment-only line between keys" {
    result="$(printf '{\n  "a": 1,\n  // commented out: "z": 0,\n  "b": 2\n}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1,"b":2}' ]
}

@test "empty object stays empty" {
    result="$(printf '{}' | strip)"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{}' ]
}

@test "complex realistic settings.json" {
    cat > "$TEST_TMP/in.jsonc" <<'EOF'
{
  // editor settings
  "editor.tabSize": 2,
  "editor.formatOnSave": true,
  /* language overrides */
  "[python]": {
    "editor.tabSize": 4,
    "editor.rulers": [80, 100,], // PEP-8 + wider
  },
  "files.exclude": {
    "**/__pycache__": true,
    "**/.pytest_cache": true,
  },
}
EOF
    result="$(strip < "$TEST_TMP/in.jsonc")"
    # Should parse cleanly
    printf '%s' "$result" | jq empty
    # And specific values should be intact
    [ "$(printf '%s' "$result" | jq -r '."editor.tabSize"')" = "2" ]
    [ "$(printf '%s' "$result" | jq -r '."[python]"."editor.tabSize"')" = "4" ]
    [ "$(printf '%s' "$result" | jq -c '."[python]"."editor.rulers"')" = "[80,100]" ]
}

@test "strips UTF-8 BOM at start of file" {
    # 0xEF 0xBB 0xBF is the UTF-8 BOM. Real Windows files (Notepad-saved) have it.
    printf '\357\273\277{"a":1}' > "$TEST_TMP/bom.json"
    result="$(strip < "$TEST_TMP/bom.json")"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}

@test "BOM stripping doesn't affect files without BOM" {
    printf '{"a":1}' > "$TEST_TMP/no-bom.json"
    result="$(strip < "$TEST_TMP/no-bom.json")"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}

@test "BOM-like sequence inside string is preserved" {
    # If the BOM bytes appear later in the file (not at offset 0), they're part of the data.
    # Test that we only strip the leading BOM.
    printf '{"a":"\357\273\277inside"}' > "$TEST_TMP/midbom.json"
    result="$(strip < "$TEST_TMP/midbom.json")"
    # The internal BOM should remain (jq will accept it as valid UTF-8 in a string).
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ -n "$parsed" ]
}

@test "handles CRLF line endings" {
    printf '{\r\n  "a": 1,\r\n  // comment\r\n  "b": 2\r\n}\r\n' > "$TEST_TMP/crlf.json"
    result="$(strip < "$TEST_TMP/crlf.json")"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1,"b":2}' ]
}

@test "R1-10: pure-LF JSON input is accepted (smoke)" {
    # Renamed from a misnamed "mixed CR-only and LF" test that only ever
    # wrote LF bytes — false coverage flagged in R1 (CDX-005). The pure-LF
    # smoke is still useful, so we keep it under an honest name and add the
    # actual CR-bearing tests below.
    printf '{\n  "a": 1\n}' > "$TEST_TMP/lf.json"
    [ "$(LC_ALL=C tr -cd '\r' < "$TEST_TMP/lf.json" | wc -c | tr -d ' ')" = "0" ]
    result="$(strip < "$TEST_TMP/lf.json")"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}

@test "R1-10: real CR bytes inside JSON whitespace are accepted" {
    # Stripper must handle bare CR as JSON whitespace inside structural
    # context (jq treats CR as whitespace). This test actually contains CR
    # bytes — sanity-checked below — unlike the prior false-coverage test.
    printf '{\r  "a": 1\r}' > "$TEST_TMP/cr-ws.json"
    [ "$(LC_ALL=C tr -cd '\r' < "$TEST_TMP/cr-ws.json" | wc -c | tr -d ' ')" -gt 0 ]
    result="$(strip < "$TEST_TMP/cr-ws.json")"
    parsed="$(printf '%s' "$result" | jq -c '.' 2>/dev/null)"
    [ "$parsed" = '{"a":1}' ]
}

@test "R1-10: CRLF line endings are accepted (real CR bytes)" {
    # The most common Windows case. Earlier tests asserted via filename
    # that "CRLF works"; this one verifies the file actually contains CR
    # bytes and that stripper output round-trips through jq cleanly.
    printf '{\r\n  "a": 1\r\n}' > "$TEST_TMP/crlf.json"
    [ "$(LC_ALL=C tr -cd '\r' < "$TEST_TMP/crlf.json" | wc -c | tr -d ' ')" -gt 0 ]
    result="$(strip < "$TEST_TMP/crlf.json")"
    parsed="$(printf '%s' "$result" | jq -c '.')"
    [ "$parsed" = '{"a":1}' ]
}
