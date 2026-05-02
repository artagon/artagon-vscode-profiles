## ADDED Requirements

### Requirement: Lean trigger description
The skill SHALL present a frontmatter `description` that fits within a single paragraph (~600 chars) and triggers on the generic intent of reading, writing, validating, or migrating VS Code-family JSONC configuration, rather than enumerating example phrasings.

#### Scenario: Description fits a single paragraph
- **WHEN** the SKILL.md frontmatter is parsed
- **THEN** the `description` field is one paragraph that names the configuration family (VS Code and forks: Cursor, Windsurf, VSCodium, Code-OSS, Insiders) and the generic intent triggers, without enumerating per-question example phrasings

#### Scenario: Description does not enumerate example user phrasings
- **WHEN** a reviewer reads the description
- **THEN** the description does not contain a literal list of example user prompts (e.g. "what does this settings.json do", "where does Cursor store keybindings"); those examples live in the SKILL.md body's `When this skill applies` section instead

### Requirement: Literal-match test assertions
The bats test helper SHALL provide assertion helpers that match needle strings literally, not via shell glob patterns, so that brackets and other shell metacharacters in test assertions do not cause false-positive passes.

#### Scenario: Bracketed needle is matched literally
- **WHEN** a test calls `assert_contains "[python]"` against output that does not contain the literal substring `[python]`
- **THEN** the test fails — even if the output contains the individual characters that would satisfy a `[python]` glob character class

#### Scenario: Negative assertion is also literal
- **WHEN** a test calls `assert_not_contains "[python]"` against output that does not contain the literal substring `[python]`
- **THEN** the test passes regardless of whether the output happens to contain characters from the bracket class

### Requirement: Locale-deterministic shell tooling
The shell tools SHALL behave identically under non-C locales (e.g. Turkish, with dotless-i collation) so that downstream callers do not see set-operation drift when running in a containerized or developer environment with a non-C `LC_ALL`.

#### Scenario: Validator under Turkish locale
- **WHEN** `vscode-jsonc-validate` is invoked with `LC_ALL=tr_TR.UTF-8 LANG=tr_TR.UTF-8` on a settings file containing both `INDEX` and `index` keys
- **THEN** its exit code and output match the C-locale invocation byte-for-byte

#### Scenario: Extensions-audit under Turkish locale
- **WHEN** `vscode-extensions-audit` is invoked under `LC_ALL=tr_TR.UTF-8` against the same allowlist + recommendations pair as a C-locale run
- **THEN** the set-difference output (NOT_ALLOWED, NOT_RECOMMENDED, IN_BOTH) matches byte-for-byte

#### Scenario: Profile-diff under Turkish locale
- **WHEN** `vscode-profile-diff` is invoked under `LC_ALL=tr_TR.UTF-8` against two profiles
- **THEN** the diff sets (only-left, only-right, changed) match the C-locale output byte-for-byte

### Requirement: NUL-byte rejection at the wrapper layer
The shell wrappers (`vscode-jsonc-validate`, `vscode-extensions-audit`, `vscode-profile-diff`) SHALL reject any input file containing raw NUL bytes BEFORE invoking `jsonc-strip.awk`, since POSIX shell command substitution silently truncates at the first NUL and would mask appended content from downstream validation, audit, and diff logic. Detection lives in `lib/nul-check.sh` (sourced by the wrappers) using a byte-count comparison via `tr -d '\0' | wc -c`, which is portable across `gawk`, `mawk`, and BSD `awk` (the latter two truncate `length()`/`substr()` at NUL).

#### Scenario: Validator rejects NUL byte
- **WHEN** `vscode-jsonc-validate` is invoked on a file with a raw `\0` byte
- **THEN** the validator exits non-zero and emits "ERROR: ... raw NUL byte" on stderr, before any awk command substitution runs

#### Scenario: Audit rejects NUL byte
- **WHEN** `vscode-extensions-audit` is invoked on an `extensions.json` containing a raw `\0` byte before a `]` so that pre-NUL bytes parse as valid JSON
- **THEN** the audit exits non-zero with the same NUL-byte error message, rather than reporting compliance based only on the pre-NUL bytes

#### Scenario: Profile-diff rejects NUL byte on either side
- **WHEN** `vscode-profile-diff` is invoked with a raw `\0` byte in either the left or right input file
- **THEN** the tool exits non-zero with the NUL-byte error identifying which file contained the byte

### Requirement: Audit and validator input-shape contracts
The shell tools SHALL confirm that their JSONC inputs match the expected JSON shape (object root, string-array `recommendations`, known MCP transport enum, well-typed `tasks`/`configurations` arrays) BEFORE running downstream value-level checks. Implicit success on shape mismatch is forbidden. Exit codes SHALL stay within each tool's documented range; raw `jq` exit codes SHALL NOT propagate to the caller.

#### Scenario: Audit rejects array-root extensions.json
- **WHEN** `vscode-extensions-audit` is invoked with an `extensions.json` whose JSONC root is a JSON array (rather than an object)
- **THEN** the audit exits non-zero with a clear shape error, NOT exit 0 — even if the allowlist would otherwise be satisfied by zero recommendations

#### Scenario: Validator rejects non-string recommendation entries
- **WHEN** `vscode-jsonc-validate --kind extensions` is invoked on a file whose `recommendations` array contains `null`, a number, or an object
- **THEN** the validator exits within its documented range (`0|1|2|3|4`) with a "recommendations entries must be strings" error, NOT a raw `jq` stack trace and exit `5`

#### Scenario: MCP type enum tracks the current MCP protocol revision
- **WHEN** `vscode-jsonc-validate --kind mcp` is invoked on a server entry whose `type` is not in the current MCP protocol's transport set (as of the 2025-03-26 revision: `stdio` and `http` only — `sse` was deprecated)
- **THEN** the validator exits non-zero with a "must be ..." enum error and the validator + `references/per-project-files.md` agree on the canonical list

#### Scenario: Wrong-shape tasks/configurations rejected within documented exit range
- **WHEN** `vscode-jsonc-validate --kind tasks` is invoked on a file with `"tasks": "not array"` (or analogously for `--kind launch` with `"configurations": "not array"`)
- **THEN** the validator exits with the documented schema-error code (2) and the schema_check error message, NOT a raw `jq` "string has no keys" trace and exit `5`

### Requirement: Documentation does not embed magic counts
The skill body SHALL NOT include hardcoded test counts or other version-coupled magic numbers in narrative documentation, since they drift silently across changes.

#### Scenario: Bundled-scripts section omits a hardcoded test count
- **WHEN** the SKILL.md "Bundled scripts" section describes the test suite
- **THEN** it refers to the suite by behavior ("covering syntax, schema, and edge cases for each tool") rather than a literal count, and points the reader at `scripts/run_tests.sh` to enumerate the suite

