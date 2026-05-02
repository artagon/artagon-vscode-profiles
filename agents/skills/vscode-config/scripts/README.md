# vscode-config scripts

Three pure-shell utilities (no Python) for working with VS Code config files, plus a
JSONC-to-JSON helper they all share.

## Tools

### `vscode-jsonc-validate`
Validates a VS Code config file. Strips JSONC comments and trailing commas, parses
with `jq`, detects duplicate top-level keys, and (when the file kind is known) checks
required schema fields.

```sh
vscode-jsonc-validate .vscode/settings.json
vscode-jsonc-validate --kind tasks build-config.json
```

Recognized kinds (auto-detected from filename or set via `--kind`): `tasks`, `launch`,
`extensions`, `settings`, `mcp`, `generic`.

Exit codes: `0` ok, `1` syntax error, `2` schema error, `3` warning (duplicate keys
or contradictory entries), `4` usage error.

### `vscode-extensions-audit`
Audits a `.vscode/extensions.json` against an allowed list of extension IDs. Reports
recommended extensions not on the list, items appearing in both `recommendations` and
`unwantedRecommendations`, and allowed-list items that are explicitly unwanted.

```sh
vscode-extensions-audit --allowed approved-extensions.txt .vscode/extensions.json
vscode-extensions-audit --allowed-inline "id.one,id.two" --quiet .vscode/extensions.json
```

The allowed-list file format: one ID per line, blank lines and `#` comments ignored.

**Important:** `extensions.json` is a *suggestion* mechanism, not enforcement. To
actually block extension installation, deploy VS Code's enterprise `AllowedExtensions`
policy. See `references/precedence.md` in the parent skill.

Exit codes: `0` clean, `1` violations, `2` usage, `3` unparseable input.

### `vscode-profile-diff`
Diffs two JSONC settings files at the key level. Ignores comment differences, key
ordering, and trailing-comma variations. Reports added, removed, changed, and (with
`--verbose`) identical keys.

```sh
vscode-profile-diff old-profile.json new-profile.json
vscode-profile-diff --json a.json b.json | jq '.changed'
```

Exit codes: `0` identical, `1` differ, `2` error.

### `jsonc-strip.awk` (helper)
The shared JSONC → JSON converter. Used by all three tools above. Handles `//` line
comments, `/* */` block comments, trailing commas in objects and arrays, and string
escapes (so a `//` inside a JSON string isn't mistaken for a comment).

Run directly with `awk -f jsonc-strip.awk < input.jsonc > output.json`.

## Dependencies

- `bash` (for `vscode-extensions-audit` and `vscode-profile-diff` — they use process
  substitution and `mapfile`-style flows that aren't POSIX). `vscode-jsonc-validate`
  is `/bin/sh`-clean.
- `jq` 1.6+ for JSON manipulation.
- `awk` (mawk, gawk, busybox awk all work).

## Tests

`bats` test suites live under `tests/`. Run all suites:

```sh
./run_tests.sh
```

Filter to a specific test:

```sh
./run_tests.sh "duplicate keys"
```

TAP output for CI:

```sh
./run_tests.sh --tap
```

Test layout (one suite per script under test):

- `tests/jsonc-strip.bats` — comments, trailing commas, BOM, CRLF, escaped quotes, edge cases
- `tests/vscode-jsonc-validate.bats` — syntax errors, schema rules per kind (tasks/launch/extensions/settings/mcp), duplicate-key detection, exit codes, NUL-byte rejection
- `tests/vscode-extensions-audit.bats` — allowed/recommended/unwanted set membership, contradictions, allowlist file vs inline forms
- `tests/vscode-profile-diff.bats` — only-left / only-right / changed sets, JSON output shape, JSONC quirks ignored

The total count is intentionally not pinned in this README — it drifts every time a test is added. Run `bash scripts/run_tests.sh --tap` to see the current count.

Each test runs in its own temp directory (`$TEST_TMP`) which is cleaned up by the
`teardown` hook, so suites are safe to run in parallel.

## Installation

These scripts have no install step beyond putting them on your `PATH`. From your
shell rc:

```sh
export PATH="$PATH:/path/to/vscode-config/scripts"
```

Or symlink individual tools into `~/.local/bin/`:

```sh
ln -s /path/to/vscode-config/scripts/vscode-jsonc-validate ~/.local/bin/
```
