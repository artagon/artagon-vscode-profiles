## ADDED Requirements

### Requirement: Test framework

The project SHALL use bats-core as its sole shell-script test framework.
Tests written in any other framework (raw bash runners, shunit2, shellspec)
MUST NOT be checked in. The project MUST document bats-core as a contributor
prerequisite in `README.md` and `CONTRIBUTING.md`.

#### Scenario: Contributor runs tests locally

- **WHEN** a contributor with bats-core installed runs `bats scripts/tests/bats/`
  from the repository root
- **THEN** every test in `scripts/tests/bats/*.bats` executes and the runner
  exits 0 if and only if all tests pass

#### Scenario: bats-core not installed

- **WHEN** a contributor without bats-core attempts to run the suite
- **THEN** they receive a clear error from the system
  (`bats: command not found`) and `CONTRIBUTING.md` documents the install
  steps for macOS, Debian/Ubuntu, and npm

### Requirement: Test layout

Tests MUST live under `scripts/tests/bats/` with one `.bats` file per
logical concern. Shared fixtures, mocks, and assertion helpers MUST live
under `scripts/tests/bats/helpers/` and be loaded via bats's `load`
directive.

Tests MUST scope new state to their per-test `$BATS_TEST_TMPDIR`
whenever feasible. Some production scripts under test
(`compose-settings.sh`, `export-profiles.sh`) intentionally write to
the working tree (`_merged/`, `exports/`, profile symlinks); tests that
exercise those scripts MAY allow them to write to the working tree, but
the suite as a whole MUST leave the working tree in the same state it
started in — verified by `git status --porcelain` returning empty after
the suite runs to completion. This means either (a) tests run scripts
against a temp repo copy under `$BATS_TEST_TMPDIR`, or (b) tests run
scripts against the live repo and either teardown reverts the writes
or `git checkout -- <paths>` restores them. Either approach is
permitted; the test author chooses.

#### Scenario: Test isolation leaves no residue in the working tree

- **WHEN** the bats suite runs to completion (pass or fail) from a clean
  working tree
- **THEN** `git status --porcelain` shows no new or modified files
  outside the source-controlled set — regardless of which isolation
  strategy individual tests used (temp copy vs live + restore)

#### Scenario: Helper reuse

- **WHEN** more than one `.bats` file needs the mocked `code` CLI stub
- **THEN** the stub lives in `scripts/tests/bats/helpers/mock-code-cli.bash`
  and is loaded via `load helpers/mock-code-cli` rather than duplicated

### Requirement: Coverage parity with legacy runner

The bats suite MUST cover every assertion present in
`scripts/tests/run.sh` at the time of migration. The migration is not
complete and `scripts/tests/run.sh` MUST NOT be deleted until a
parity-checkpoint task verifies that each numbered assertion below has a
corresponding bats test (or a documented superseding test):

1. `validate-json.sh` exits 0 on the live tree.
2. `compose-settings.sh` exits 0 on the live tree.
3. `profiles/java-profile-crisp/settings.json` is a symlink after compose.
4. `export-profiles.sh` produces a non-empty
   `exports/java-profile-crisp.code-profile`.
5. `open-profiles.sh` invokes the mocked `code` CLI exactly once per
   profile (with `VSCODE_SKIP_EXTENSION_INSTALL=1`).
6. The cache layer in `open-profiles.sh` skips installs on the second
   invocation when the cache hash matches.
7. `install-extensions.sh ... --group AI` installs only AI-classified
   extensions (count derived from the profile's `extensions.json`).
8. `compose-settings.sh` rejects `@extends` traversal paths
   (`../../../etc/passwd`), absolute paths (`/tmp/evil.jsonc`),
   home-prefixed paths (`~/secret.jsonc`), malformed `@extends` values
   (non-string, non-array), and cycles introduced via symlink.
9. `install-extensions.sh`, `import-profile.sh`, and `vspcli --install-ext`
   each reject extension IDs containing shell injection metacharacters
   with a `rejected extension id` message and a non-zero exit.
10. The `import-profile.sh` PROFILE_ID generator falls back to
    `python3 -c 'import secrets; print(secrets.token_hex(4))'` when
    `openssl rand -hex 4` fails on PATH, and produces an 8-char lowercase
    hex string.
11. The `import-profile.sh` PROFILE_ID generator runs cleanly under
    `set -euo pipefail`.
12. The `EXTENSION_ID_REGEX` literal in `scripts/lib/extension-id.sh`
    appears verbatim in the live capability specs for
    `install-profile-extensions`, `import-profile-bundles`, and
    `manage-profile-cli`. The bats test MUST resolve each capability's
    spec by checking `openspec/specs/<cap>/spec.md` first, then falling
    back to the still-active change folder
    `openspec/changes/harden-profile-tooling-and-pipeline/specs/<cap>/spec.md`.
    This makes the test resilient to that change being archived
    (post-archive, the canonical location moves from `changes/` to
    `specs/`).
13. Every `<stack>-{crisp,retina}.jsonc` leaf under `_overrides/` for
    `java-{gradle,maven,profile,spring}` and `rust-profile` is a symlink
    pointing to `<stack>-base.jsonc`.
14. Every merged JSON file in `_merged/` has
    `security.workspace.trust.untrustedFiles == "prompt"`.

#### Scenario: Parity check before legacy deletion

- **WHEN** the migration's parity-checkpoint task runs
- **THEN** every assertion 1–14 above is mapped to a bats test (file +
  test name), and any unmapped assertion blocks deletion of
  `scripts/tests/run.sh`

### Requirement: Mechanized parity verification

The migration MUST include an executable parity verifier — not only a
manual review — for the duration of the side-by-side period. The
parity file `openspec/changes/migrate-tests-to-bats/parity.md` MUST
use a fixed-shape line format (one assertion per line matching
`^N\. <file>:<test-name>$`, where N is 1–14). Lines that don't match
the regex are ignored as commentary. The verifier reads `parity.md`,
extracts each numbered line's bats `file:test-name` reference, and
asserts that the file exists and contains a `@test` declaration with
that exact name. The verifier MUST exit non-zero on any mismatch. CI
MUST run the verifier on every PR while the migration is in flight.
The verifier and its CI step are removed in the same commit that
deletes `scripts/tests/run.sh`.

#### Scenario: Verifier catches a missing bats test

- **WHEN** `parity.md` claims assertion N is covered by
  `<file>.bats:<test-name>` but `<file>.bats` either does not exist or
  contains no `@test "<test-name>"` declaration
- **THEN** the verifier exits non-zero with a message naming the
  missing pair, and CI fails

#### Scenario: Verifier passes on a complete parity file

- **WHEN** every numbered line in `parity.md` resolves to an existing
  `@test` in the named bats file
- **THEN** the verifier exits 0

### Requirement: Shell-script linting

The CI workflow MUST run `shellcheck` against every `*.sh` under
`scripts/` and every `*.bash` helper under
`scripts/tests/bats/helpers/`. The lint step MUST fail the build on
any warning at severity `--severity=warning` or higher. Helper files
MUST declare their shell dialect via a `# shellcheck shell=bash`
directive at the top of the file (helpers are sourced, not run, so
shellcheck cannot infer dialect from a shebang). The shellcheck step
is independent of the bats migration and persists after
`scripts/tests/run.sh` is deleted.

#### Scenario: New script with shellcheck violation blocks merge

- **WHEN** a pull request adds a `*.sh` script with a shellcheck
  warning (e.g., SC2086 unquoted variable expansion)
- **THEN** the CI shellcheck step exits non-zero and the PR cannot
  merge until the warning is resolved or explicitly disabled with a
  justified `# shellcheck disable=SC2086` directive

#### Scenario: Helper without shell directive is rejected

- **WHEN** a new file under `scripts/tests/bats/helpers/*.bash` lacks
  the `# shellcheck shell=bash` directive
- **THEN** shellcheck either skips the file (no lint coverage —
  silent regression) or warns about missing dialect declaration. The
  CI configuration MUST treat the latter as a failure

### Requirement: CI invocation

The CI workflow MUST install bats-core before running tests and MUST
invoke the suite via `bats scripts/tests/bats/` (not via the legacy
runner) once the migration is complete. CI MUST fail on any failing test.

#### Scenario: CI runs the bats suite

- **WHEN** a pull request triggers `.github/workflows/ci.yml`
- **THEN** the workflow installs bats-core, runs `bats scripts/tests/bats/`,
  and the job fails if any test reports non-zero exit

### Requirement: Legacy runner deletion

`scripts/tests/run.sh` MUST be deleted as the final task of the
migration, only after the parity-checkpoint passes. Deletion MUST be
in the same change as the bats suite landing — a follow-up "cleanup"
change is NOT acceptable.

#### Scenario: Legacy runner removed at parity

- **WHEN** the migration completes
- **THEN** `scripts/tests/run.sh` no longer exists in the repository,
  and `.github/workflows/ci.yml` no longer references it
