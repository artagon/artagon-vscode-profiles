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

### Requirement: NUL-byte rejection in JSONC stripper
The shared `jsonc-strip.awk` SHALL reject any input containing raw NUL bytes, since shell command substitution silently truncates at the first NUL and would mask appended content from validation, audit, and diff tools.

#### Scenario: Stripper rejects NUL byte
- **WHEN** input containing a raw `\0` byte is piped to `jsonc-strip.awk`
- **THEN** the stripper exits non-zero and emits an error message on stderr identifying the NUL-byte cause

#### Scenario: Validator surfaces NUL-byte rejection
- **WHEN** `vscode-jsonc-validate` is invoked on a file with a raw `\0` byte
- **THEN** the validator exits non-zero and surfaces the underlying NUL-byte error rather than producing an "OK" verdict

#### Scenario: Audit surfaces NUL-byte rejection
- **WHEN** `vscode-extensions-audit` is invoked on an `extensions.json` containing a raw `\0` byte before a `]` so that pre-NUL bytes parse as valid JSON
- **THEN** the audit exits non-zero rather than reporting compliance based on only the pre-NUL bytes

### Requirement: Documentation does not embed magic counts
The skill body SHALL NOT include hardcoded test counts or other version-coupled magic numbers in narrative documentation, since they drift silently across changes.

#### Scenario: Bundled-scripts section omits a hardcoded test count
- **WHEN** the SKILL.md "Bundled scripts" section describes the test suite
- **THEN** it refers to the suite by behavior ("covering syntax, schema, and edge cases for each tool") rather than a literal count, and points the reader at `scripts/run_tests.sh` to enumerate the suite

