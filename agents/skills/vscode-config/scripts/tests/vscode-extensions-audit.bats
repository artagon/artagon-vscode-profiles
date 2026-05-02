#!/usr/bin/env bats
#
# Tests for vscode-extensions-audit.

load test_helper

# Helper to write the standard allowed list.
write_allowed() {
    cat > "$TEST_TMP/allowed.txt" <<'EOF'
# Approved extensions
dbaeumer.vscode-eslint
esbenp.prettier-vscode
ms-python.python
EOF
}

@test "compliant extensions.json exits 0" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["dbaeumer.vscode-eslint", "esbenp.prettier-vscode"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" "$TEST_TMP/ext.json"
    assert_exit 0
    assert_contains "OK"
}

@test "rogue extension flagged with VIOLATION" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["dbaeumer.vscode-eslint", "rogue.banned-ext"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" "$TEST_TMP/ext.json"
    assert_exit 1
    assert_contains "VIOLATION"
    assert_contains "rogue.banned-ext"
}

@test "extension in both lists flagged" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["dbaeumer.vscode-eslint"],
  "unwantedRecommendations": ["dbaeumer.vscode-eslint"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" "$TEST_TMP/ext.json"
    assert_exit 1
    assert_contains "in both recommendations and unwantedRecommendations"
}

@test "allowed-list extension marked unwanted is flagged" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["esbenp.prettier-vscode"],
  "unwantedRecommendations": ["dbaeumer.vscode-eslint"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" "$TEST_TMP/ext.json"
    assert_exit 1
    assert_contains "allowed-list extensions explicitly marked unwanted"
    assert_contains "dbaeumer.vscode-eslint"
}

@test "--allowed-inline accepts comma-separated list" {
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["a.b", "c.d"]
}
EOF
    run "$AUDIT" --allowed-inline "a.b,c.d" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
}

@test "--quiet suppresses INFO output" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["dbaeumer.vscode-eslint"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
    assert_not_contains "INFO"
    assert_not_contains "OK"
}

@test "informational shows allowed not recommended (without --quiet)" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["dbaeumer.vscode-eslint"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" "$TEST_TMP/ext.json"
    assert_exit 0
    assert_contains "INFO"
    assert_contains "esbenp.prettier-vscode"
    assert_contains "ms-python.python"
}

@test "comments and blank lines in allowed list are ignored" {
    cat > "$TEST_TMP/allowed.txt" <<'EOF'
# This is a header comment

dbaeumer.vscode-eslint

# section break
esbenp.prettier-vscode
EOF
    write_file "$TEST_TMP/ext.json" <<'EOF'
{ "recommendations": ["dbaeumer.vscode-eslint"] }
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
}

@test "empty extensions.json passes (nothing to audit)" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
}

@test "JSONC comments in extensions.json are handled" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  // Required tooling
  "recommendations": [
    "dbaeumer.vscode-eslint", // ESLint
    "esbenp.prettier-vscode"  /* Prettier */
  ],
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
}

@test "missing both --allowed flags exits 2" {
    write_file "$TEST_TMP/ext.json" '{}'
    run "$AUDIT" "$TEST_TMP/ext.json"
    assert_exit 2
}

@test "missing extensions.json file exits 2" {
    write_allowed
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" /nonexistent.json
    assert_exit 2
}

@test "unparseable extensions.json exits 3" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{ this is not json }
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" "$TEST_TMP/ext.json"
    assert_exit 3
    assert_contains "not valid JSONC"
}

@test "passing both --allowed and --allowed-inline exits 2" {
    write_allowed
    write_file "$TEST_TMP/ext.json" '{}'
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --allowed-inline "a.b" "$TEST_TMP/ext.json"
    assert_exit 2
}

@test "multiple violations are all reported" {
    write_allowed
    write_file "$TEST_TMP/ext.json" <<'EOF'
{
  "recommendations": ["rogue.one", "rogue.two", "dbaeumer.vscode-eslint"]
}
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --quiet "$TEST_TMP/ext.json"
    assert_exit 1
    assert_contains "rogue.one"
    assert_contains "rogue.two"
    assert_not_contains "dbaeumer.vscode-eslint" # this one IS allowed
}

@test "allowed list with CRLF line endings works" {
    # Files edited on Windows often have CRLF. The trailing \r must not break matching.
    printf 'dbaeumer.vscode-eslint\r\nesbenp.prettier-vscode\r\n' > "$TEST_TMP/allowed-crlf.txt"
    write_file "$TEST_TMP/ext.json" <<'EOF'
{ "recommendations": ["dbaeumer.vscode-eslint", "esbenp.prettier-vscode"] }
EOF
    run "$AUDIT" --allowed "$TEST_TMP/allowed-crlf.txt" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
}

@test "allowed-inline with CR characters tolerated" {
    write_file "$TEST_TMP/ext.json" '{"recommendations": ["a.b"]}'
    # If a user pipes a file via shell into --allowed-inline and forgets to strip CRs,
    # we should still match.
    run "$AUDIT" --allowed-inline "$(printf 'a.b\r,c.d')" --quiet "$TEST_TMP/ext.json"
    assert_exit 0
}

@test "extensions.json with UTF-8 BOM is accepted" {
    write_allowed
    printf '\357\273\277{"recommendations":["dbaeumer.vscode-eslint"]}' > "$TEST_TMP/bom-ext.json"
    run "$AUDIT" --allowed "$TEST_TMP/allowed.txt" --quiet "$TEST_TMP/bom-ext.json"
    assert_exit 0
}
