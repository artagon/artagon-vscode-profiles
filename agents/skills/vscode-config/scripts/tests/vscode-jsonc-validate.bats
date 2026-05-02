#!/usr/bin/env bats
#
# Tests for vscode-jsonc-validate.

load test_helper

# ---------- generic / settings ----------

@test "valid settings.json exits 0" {
    write_file "$TEST_TMP/settings.json" <<'EOF'
{
  // basic settings
  "editor.tabSize": 2,
  "editor.formatOnSave": true,
}
EOF
    run "$VALIDATE" "$TEST_TMP/settings.json"
    assert_exit 0
    assert_contains "OK"
    assert_contains "(settings)"
}

@test "missing comma between keys exits 1 with syntax error message" {
    write_file "$TEST_TMP/broken.json" <<'EOF'
{
  "a": 1
  "b": 2
}
EOF
    run "$VALIDATE" --kind settings "$TEST_TMP/broken.json"
    assert_exit 1
    assert_contains "not valid JSONC"
}

@test "duplicate top-level keys exits 3" {
    write_file "$TEST_TMP/dupe.json" <<'EOF'
{
  "editor.tabSize": 2,
  "editor.fontSize": 14,
  "editor.tabSize": 4
}
EOF
    run "$VALIDATE" --kind settings "$TEST_TMP/dupe.json"
    assert_exit 3
    assert_contains "duplicate top-level keys"
    assert_contains "editor.tabSize"
}

@test "trailing comma alone is fine (it's JSONC)" {
    write_file "$TEST_TMP/ok.json" <<'EOF'
{
  "a": 1,
  "b": 2,
}
EOF
    run "$VALIDATE" --kind settings "$TEST_TMP/ok.json"
    assert_exit 0
}

@test "kind is auto-detected from filename" {
    write_file "$TEST_TMP/tasks.json" <<'EOF'
{
  "version": "2.0.0",
  "tasks": []
}
EOF
    run "$VALIDATE" "$TEST_TMP/tasks.json"
    assert_exit 0
    assert_contains "(tasks)"
}

@test "explicit --kind overrides filename inference" {
    write_file "$TEST_TMP/random.json" <<'EOF'
{
  "version": "2.0.0",
  "tasks": []
}
EOF
    run "$VALIDATE" --kind tasks "$TEST_TMP/random.json"
    assert_exit 0
    assert_contains "(tasks)"
}

@test "missing file exits 4" {
    run "$VALIDATE" /nonexistent/path.json
    assert_exit 4
}

@test "missing argument prints usage and exits 4" {
    run "$VALIDATE"
    assert_exit 4
}

# ---------- tasks.json ----------

@test "tasks.json with version 0.1.0 exits 2" {
    write_file "$TEST_TMP/tasks.json" <<'EOF'
{
  "version": "0.1.0",
  "tasks": []
}
EOF
    run "$VALIDATE" "$TEST_TMP/tasks.json"
    assert_exit 2
    assert_contains "must declare \"version\": \"2.0.0\""
}

@test "tasks.json without tasks array exits 2" {
    write_file "$TEST_TMP/tasks.json" <<'EOF'
{
  "version": "2.0.0"
}
EOF
    run "$VALIDATE" "$TEST_TMP/tasks.json"
    assert_exit 2
    assert_contains "must have a \"tasks\" array"
}

@test "tasks.json with valid task passes" {
    write_file "$TEST_TMP/tasks.json" <<'EOF'
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "build",
      "type": "shell",
      "command": "npm run build"
    }
  ]
}
EOF
    run "$VALIDATE" "$TEST_TMP/tasks.json"
    assert_exit 0
}

@test "tasks.json with task missing label exits 2" {
    write_file "$TEST_TMP/tasks.json" <<'EOF'
{
  "version": "2.0.0",
  "tasks": [
    { "type": "shell", "command": "echo hi" }
  ]
}
EOF
    run "$VALIDATE" "$TEST_TMP/tasks.json"
    assert_exit 2
    assert_contains "missing or empty \"label\""
}

# ---------- launch.json ----------

@test "launch.json with version 0.2.0 and valid config passes" {
    write_file "$TEST_TMP/launch.json" <<'EOF'
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Debug",
      "type": "node",
      "request": "launch",
      "program": "${workspaceFolder}/index.js"
    }
  ]
}
EOF
    run "$VALIDATE" "$TEST_TMP/launch.json"
    assert_exit 0
}

@test "launch.json with wrong version exits 2" {
    write_file "$TEST_TMP/launch.json" <<'EOF'
{
  "version": "2.0.0",
  "configurations": []
}
EOF
    run "$VALIDATE" "$TEST_TMP/launch.json"
    assert_exit 2
    assert_contains "must declare \"version\": \"0.2.0\""
}

@test "launch.json with config missing 'request' exits 2" {
    write_file "$TEST_TMP/launch.json" <<'EOF'
{
  "version": "0.2.0",
  "configurations": [
    { "name": "Debug", "type": "node" }
  ]
}
EOF
    run "$VALIDATE" "$TEST_TMP/launch.json"
    assert_exit 2
    assert_contains "missing \"request\""
}

@test "launch.json with invalid request value exits 2" {
    write_file "$TEST_TMP/launch.json" <<'EOF'
{
  "version": "0.2.0",
  "configurations": [
    { "name": "Debug", "type": "node", "request": "invalid" }
  ]
}
EOF
    run "$VALIDATE" "$TEST_TMP/launch.json"
    assert_exit 2
    assert_contains "must be \"launch\" or \"attach\""
}

# ---------- extensions.json ----------

@test "extensions.json with valid IDs passes" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{
  "recommendations": [
    "dbaeumer.vscode-eslint",
    "esbenp.prettier-vscode"
  ]
}
EOF
    run "$VALIDATE" "$TEST_TMP/extensions.json"
    assert_exit 0
}

@test "extensions.json with malformed ID exits 2" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{
  "recommendations": ["no-publisher-prefix"]
}
EOF
    run "$VALIDATE" "$TEST_TMP/extensions.json"
    assert_exit 2
    assert_contains "is not a valid <publisher>.<name>"
}

@test "extensions.json with same ID in both lists warns" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{
  "recommendations": ["dbaeumer.vscode-eslint"],
  "unwantedRecommendations": ["dbaeumer.vscode-eslint"]
}
EOF
    run "$VALIDATE" "$TEST_TMP/extensions.json"
    # Exit 3 = warning level (per script docs). No malformed ID, so error path not hit.
    assert_exit 3
    assert_contains "appears in both"
}

@test "extensions.json with empty recommendations passes" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{}
EOF
    run "$VALIDATE" "$TEST_TMP/extensions.json"
    assert_exit 0
}

# ---------- mcp.json ----------

@test "mcp.json with valid stdio server passes" {
    write_file "$TEST_TMP/mcp.json" <<'EOF'
{
  "servers": {
    "myServer": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@example/mcp-server"]
    }
  }
}
EOF
    run "$VALIDATE" "$TEST_TMP/mcp.json"
    assert_exit 0
}

@test "mcp.json stdio server without command exits 2" {
    write_file "$TEST_TMP/mcp.json" <<'EOF'
{
  "servers": {
    "broken": {
      "type": "stdio"
    }
  }
}
EOF
    run "$VALIDATE" "$TEST_TMP/mcp.json"
    assert_exit 2
    assert_contains "stdio servers require \"command\""
}

@test "mcp.json http server without url exits 2" {
    write_file "$TEST_TMP/mcp.json" <<'EOF'
{
  "servers": {
    "broken": {
      "type": "http"
    }
  }
}
EOF
    run "$VALIDATE" "$TEST_TMP/mcp.json"
    assert_exit 2
    assert_contains "http servers require \"url\""
}

@test "mcp.json with invalid type exits 2" {
    write_file "$TEST_TMP/mcp.json" <<'EOF'
{
  "servers": {
    "broken": {
      "type": "websocket"
    }
  }
}
EOF
    run "$VALIDATE" "$TEST_TMP/mcp.json"
    assert_exit 2
    assert_contains "must be stdio or http"
}

@test "R1-03: mcp.json with sse transport is rejected (deprecated by MCP 2025-03-26)" {
    write_file "$TEST_TMP/mcp.json" <<'EOF'
{
  "servers": {
    "legacySse": {
      "type": "sse",
      "url": "https://example.com/mcp"
    }
  }
}
EOF
    run "$VALIDATE" "$TEST_TMP/mcp.json"
    assert_exit 2
    assert_contains "must be stdio or http"
}

# ---------- JSONC edge cases ----------

@test "comments in file don't trigger false syntax errors" {
    write_file "$TEST_TMP/settings.json" <<'EOF'
{
  /* multi
     line
     comment */
  "a": 1, // trailing line comment
  "b": "value with // pseudo-comment in string"
}
EOF
    run "$VALIDATE" "$TEST_TMP/settings.json"
    assert_exit 0
}

@test "file with UTF-8 BOM is accepted" {
    # Notepad on Windows saves with BOM. The validator should not flag this as invalid.
    printf '\357\273\277{"editor.tabSize":2}' > "$TEST_TMP/bom-settings.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/bom-settings.json"
    assert_exit 0
}

@test "file with CRLF line endings is accepted" {
    printf '{\r\n  "a": 1,\r\n  "b": 2\r\n}\r\n' > "$TEST_TMP/crlf-settings.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/crlf-settings.json"
    assert_exit 0
}

@test "empty file is reported clearly" {
    : > "$TEST_TMP/empty.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/empty.json"
    assert_exit 1
    assert_contains "empty"
}

@test "file with only comments is reported as empty" {
    write_file "$TEST_TMP/just-comments.json" <<'EOF'
// no actual JSON in here
/* just commentary */
EOF
    run "$VALIDATE" --kind settings "$TEST_TMP/just-comments.json"
    assert_exit 1
    assert_contains "empty"
}

# ---------- NUL-byte rejection (CR-005) ----------

@test "rejects file containing raw NUL byte" {
    # The shell-level NUL guard catches this BEFORE awk's command substitution
    # silently truncates the input. mawk and BSD awk both treat NUL as a
    # C-string terminator inside length()/substr(), so detection in awk is
    # unreliable; the guard lives in scripts/lib/nul-check.sh and runs first.
    printf '{"a":1}\0{"hidden":true}' > "$TEST_TMP/with-nul.json"
    run "$VALIDATE" "$TEST_TMP/with-nul.json"
    [ "$status" -ne 0 ]
    assert_contains "raw NUL byte"
}

@test "R1-11: accepts file with escaped \\u0000 (six-char JSON escape, not raw NUL)" {
    # Real JSON-encoded U+0000 is six characters: backslash, u, 0, 0, 0, 0.
    # The runtime guard must distinguish this (valid) from a raw 0x00 byte
    # (invalid - silently truncated by command substitution).
    # We use printf with explicit hex escape \\x5c to write the bare backslash,
    # avoiding any shell or bats preprocessor interpretation that would
    # collapse a literal backslash in the source string.
    printf '{"a":"\x5cu0000"}' > "$TEST_TMP/escaped-nul.json"
    # Sanity-check: the fixture must contain exactly one backslash and zero
    # raw NUL bytes. If a future change re-introduces a NUL here, these
    # assertions fire before the validator does.
    [ "$(LC_ALL=C tr -cd '\\' < "$TEST_TMP/escaped-nul.json" | LC_ALL=C wc -c | tr -d ' ')" = "1" ]
    [ "$(LC_ALL=C tr -cd '\0' < "$TEST_TMP/escaped-nul.json" | LC_ALL=C wc -c | tr -d ' ')" = "0" ]
    run "$VALIDATE" --kind settings "$TEST_TMP/escaped-nul.json"
    assert_exit 0
}
# ---------- R1 fixes: input-shape contracts (R1-02 validator side, R1-08) ----------

@test "R1-02 (validator): recommendations with null entry exits 2 (documented), not 5 (raw jq)" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{ "recommendations": ["dbaeumer.vscode-eslint", null] }
EOF
    run "$VALIDATE" --kind extensions "$TEST_TMP/extensions.json"
    # Must be in the documented exit-code range 0|1|2|3|4 — NOT jq's exit 5.
    assert_exit 2
    assert_contains "recommendations entries must be strings"
}

@test "R1-02 (validator): recommendations with numeric entry exits 2 with actionable message" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{ "recommendations": [42] }
EOF
    run "$VALIDATE" --kind extensions "$TEST_TMP/extensions.json"
    assert_exit 2
    assert_contains "recommendations entries must be strings"
}

@test "R1-02 (validator): recommendations with object entry exits 2 with actionable message" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{ "recommendations": [{"id": "smuggled.thing"}] }
EOF
    run "$VALIDATE" --kind extensions "$TEST_TMP/extensions.json"
    assert_exit 2
    assert_contains "recommendations entries must be strings"
}

@test "R1-02 (validator): unwantedRecommendations with null entry exits 2" {
    write_file "$TEST_TMP/extensions.json" <<'EOF'
{ "unwantedRecommendations": [null] }
EOF
    run "$VALIDATE" --kind extensions "$TEST_TMP/extensions.json"
    assert_exit 2
    assert_contains "unwantedRecommendations entries must be strings"
}

# ---------- R1 fixes: parser false-positives (R1-06, R1-07, R1-08) ----------

@test "R1-06: unterminated /* block comment is rejected" {
    # Before R1: stripper silently swallows from /* through EOF; validator
    # reports OK exit 0 for malformed JSONC. Now the awk END block detects
    # the open in_block_comment flag.
    printf '%s' '{"a":1} /* unterminated' > "$TEST_TMP/badcomment.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/badcomment.json"
    [ "$status" -ne 0 ]
    assert_contains "unterminated /* block comment"
}

@test "R1-07: escaped duplicate keys (\"a\" vs \"\\u0061\") are detected" {
    # Before R1: detector compared raw source key text, so the two keys
    # were considered distinct. jq still last-wins them, leaving a silent
    # shadow. Now keys are decoded via jq fromjson before sort/uniq.
    printf '{"a":1,"a":2}' > "$TEST_TMP/dupe-escape.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/dupe-escape.json"
    assert_exit 3
    assert_contains "duplicate top-level keys"
    assert_contains "a"
}

@test "R1-08: wrong-shape tasks ('not array') exits 2 with schema error, not raw jq 5" {
    write_file "$TEST_TMP/tasks.json" <<'EOF'
{ "version": "2.0.0", "tasks": "not array" }
EOF
    run "$VALIDATE" --kind tasks "$TEST_TMP/tasks.json"
    assert_exit 2
    assert_contains "tasks.json must have a \"tasks\" array"
}

@test "R1-08: wrong-shape configurations ('not array') exits 2 with schema error" {
    write_file "$TEST_TMP/launch.json" <<'EOF'
{ "version": "0.2.0", "configurations": "not array" }
EOF
    run "$VALIDATE" --kind launch "$TEST_TMP/launch.json"
    assert_exit 2
    assert_contains "launch.json must have a \"configurations\" array"
}

@test "R1-17: duplicate-key warning shows occurrence count" {
    printf '{"k":1,"k":2,"k":3,"k":4}' > "$TEST_TMP/multi-dup.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/multi-dup.json"
    assert_exit 3
    assert_contains "k (appears 4 times; 3 earlier values shadowed)"
}

# ---------- R1-12: locale-deterministic shell tooling ----------

@test "R1-12: validates correctly under non-C locale (Turkish)" {
    # Turkish dotless-i collation is the canonical "broke set ops" failure.
    # If anyone removes `export LC_ALL=C` from the script, this fires.
    write_file "$TEST_TMP/settings.json" <<'EOF'
{
  "INDEX": 1,
  "index": 2,
  "editor.tabSize": 4
}
EOF
    LC_ALL=tr_TR.UTF-8 LANG=tr_TR.UTF-8 run "$VALIDATE" --kind settings "$TEST_TMP/settings.json"
    assert_exit 0
    assert_contains "OK"
}

@test "R1-13: validator NUL test already covers the wrapper layer" {
    # Smoke that the NUL guard fires correctly here too — duplicated for clarity
    # alongside R1-13 audit and profile-diff coverage.
    printf '{"a":1}\0{"hidden":true}' > "$TEST_TMP/with-nul.json"
    run "$VALIDATE" --kind settings "$TEST_TMP/with-nul.json"
    [ "$status" -ne 0 ]
    assert_contains "raw NUL byte"
}
