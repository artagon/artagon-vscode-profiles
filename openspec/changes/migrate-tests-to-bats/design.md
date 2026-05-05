## Context

`scripts/tests/run.sh` (294 lines, 14 distinct assertions) is the project's
sole test surface. It runs in CI via a single line in
`.github/workflows/ci.yml` and is intended to be runnable locally with
`bash scripts/tests/run.sh`.

The runner has accumulated load-bearing complexity that the file itself
does not surface:

- Inline mocks: a 33-line `code` CLI stub built into a `$TMP/bin` directory
  and prepended to PATH (lines 12–48).
- Inline fixture builders: a self-contained mini-repo built under
  `$TMP/repo` to test compose-settings's `@extends` containment rules
  (lines 109–117).
- Trap chaining: an early `trap 'rm -rf "$TMP"' EXIT` is later replaced
  with `trap 'rm -rf "$TMP" "$INJ_PROFILE_DIR"' EXIT` (line 181) to clean
  up a fixture written into the live profiles tree.
- Shared global env: `CODE_LOG`, `VSCODE_EXTENSION_INSTALL_DELAY`,
  `VSCODE_SKIP_EXTENSION_INSTALL`, `VSCODE_USER_DIR` are set/unset
  imperatively across tests; one test's leakage can silently affect the
  next.
- One-shot reporting: any failure calls `fail()` which exits the process,
  hiding all subsequent failures and forcing repeated CI cycles to
  surface them.

The test surface is going to grow — the upcoming `profile-graceful-defaults`
change adds a regression test for `import-profile.sh`'s theme-fallback
rewrite, and the wshobson shell-testing skill recommends bats-core as the
canonical framework. Migrating now (14 assertions) is meaningfully
cheaper than later (uncapped growth).

## Goals / Non-Goals

**Goals:**

- Replace `scripts/tests/run.sh` with a bats-core suite at full coverage
  parity.
- Establish the test layout (`scripts/tests/bats/<concern>.bats` +
  `scripts/tests/bats/helpers/`) as the place all future tests are added.
- Extract reusable helpers (mocked `code` CLI, fixture-tree builders,
  `assert_compose_rejects` and similar) into helper files loaded via
  bats's `load`.
- Update CI to install bats-core and invoke the suite via `bats
  scripts/tests/bats/`.
- Document the new contributor flow in `README.md` and `CONTRIBUTING.md`.

**Non-Goals:**

- Adding new test coverage. This change preserves existing assertions
  exactly. Coverage *additions* belong in the changes that introduce the
  features being tested (e.g., `profile-graceful-defaults`).
- Switching to a different framework (shellspec, shunit2, custom). The
  project's shell-testing guidance documents bats-core as the default.
- Rewriting the production scripts under test. They are the system under
  test, not part of the migration.
- Parallelization of CI. Bats supports `--jobs N` and the suite is
  parallel-safe by design (per-test `$BATS_TEST_TMPDIR`), but enabling
  `--jobs` in CI is a separate, low-risk follow-up.

## Decisions

### Decision 1: bats-core over shellspec, shunit2, custom

**Choice**: bats-core.

**Rationale**: TAP output for CI parsing, largest ecosystem (Homebrew/nvm
test suites use it), first-class GitHub Actions integration via
`bats-core/bats-action`, parallel execution, active maintenance. The
project's `artagon-shell:shell-testing` and
`wshobson-shell-scripting--bats-testing-patterns` skills both name
bats-core as the default.

**Alternatives considered**:

- *shellspec*: better mocking, broader shell support (dash, ksh, zsh).
  All the production scripts are bash with `#!/usr/bin/env bash`, so
  cross-shell support is unused; the BDD syntax adds learning cost
  without proportional benefit here.
- *shunit2*: deprecated in the shell-testing skill; no TAP, no parallel
  execution, last release 2020.
- *Stay on `run.sh` and refactor it*: the file would need an assertion
  registry, a per-test isolation model, and TAP output — i.e., reinvent
  bats. Not worth it.

### Decision 2: One `.bats` file per logical concern, not per script

**Choice**: file boundaries follow the concerns in `run.sh` (validate,
compose, export, open, install-extensions, extension-id-allowlist,
import-profile, symlink-invariants, workspace-trust), not the production
scripts under test.

**Rationale**: tests in `run.sh` already cluster by concern. Multiple
scripts (e.g., extension-id allowlist) participate in a single test —
splitting by script would require duplicating the test or arbitrarily
choosing a "primary" script.

**Alternatives considered**:

- *One `.bats` per production script*: forces unnatural test placement
  for cross-script concerns.
- *One giant `.bats`*: defeats bats's per-file `setup_file` lifecycle
  and makes parallel-by-file execution useless.

### Decision 3: Helpers extracted into `helpers/`

**Choice**: three helper files —
`helpers/setup.bash` (ROOT detection, common env), `helpers/mock-code-cli.bash`
(the `code` stub), `helpers/assertions.bash` (`assert_compose_rejects`
and similar).

**Rationale**: the mocked `code` CLI is needed by at least four `.bats`
files (open, install-extensions, extension-id, import-profile).
Duplicating its 33 lines across files is exactly the maintenance burden
the migration is supposed to eliminate.

**Alternatives considered**:

- *Inline the stub in each file*: rejected; duplication.
- *Use `bats-mock`*: full mocking framework, but our needs are limited to
  one stub. Adding a vendored dependency for one stub is overkill.

### Decision 4: Side-by-side migration, then delete in the same change

**Choice**: keep `scripts/tests/run.sh` running in CI alongside the
growing bats suite during migration. Delete `run.sh` and switch CI to
bats only as the final task.

**Rationale**: this mirrors the
spec's "Coverage parity" requirement — every assertion in `run.sh` must
have a bats counterpart before deletion. Running both lets us spot a
bats test that *passes* when its `run.sh` counterpart *fails*, which
would be a silent coverage regression.

**Alternatives considered**:

- *Big-bang switchover*: faster but riskier; a missed assertion lands
  silently.
- *Two changes (add bats, then delete `run.sh`)*: the spec explicitly
  forbids this — a "cleanup" follow-up is rejected because it almost
  always slips and lets two test surfaces coexist indefinitely.

### Decision 5: Tighten exit-code assertions during migration

**Choice**: each bats test asserts a *specific* exit code (`[ "$status" -eq 1 ]`,
`[ "$status" -eq 0 ]`, etc.) — not the looser `-ne 0` pattern that `run.sh`
uses (`run.sh:130, 190, 208, 216`).

**Rationale**: the
`artagon-shell:shell-testing` and `wshobson-shell-scripting--bats-testing-patterns`
skills both call this out: "Exit codes matter — explicitly assert
`[ "$status" -eq N ]` rather than just `[ "$status" -ne 0 ]`." Preserving
the loose check would carry forward a known weak assertion. The cost is
modest: each migrating test author looks up the actual exit code (most
production scripts use plain `exit 1` for failure paths) and writes
the specific number.

**Alternatives considered**:

- *Strict parity (`-ne 0`)*: simpler migration but inherits a known weak
  assertion. The migration is the natural moment to tighten.
- *Defer to a follow-up*: would mean a third PR after the side-by-side
  delete. Adds a fourth PR to the bounded window in the migration plan.

### Decision 6: bats-core install in CI via `bats-core/bats-action`

**Choice**: use the official `bats-core/bats-action@3.0.0` GitHub Action
in CI. For local install, document `brew install bats-core`,
`apt install bats`, and `npm install -g bats`.

**Rationale**: it's the canonical install path documented by
bats-core. Vendoring bats into the repo (via `git submodule` or copy)
adds maintenance burden for negligible benefit.

**Alternatives considered**:

- *Vendor bats-core (copy or submodule)*: the wshobson skill mentions
  this as an option. Either form adds a maintenance burden — keeping
  the vendored copy current with upstream — for negligible benefit
  over the install action. Net cost > net benefit.

## Risks / Trade-offs

- **[Coverage regression during migration]** A bats test could pass
  while its `run.sh` counterpart fails (or vice versa) due to subtle
  isolation differences. → Mitigation: run both suites in CI for the
  duration of the migration. The parity-checkpoint task explicitly
  cross-references each `run.sh` assertion to a bats file:test-name
  before allowing `run.sh` deletion.

- **[Mocked `code` CLI behavior drift]** If a `.bats` file forgets to
  load `helpers/mock-code-cli.bash`, the test will accidentally invoke
  the real `code` if installed (or fail confusingly if not). →
  Mitigation: helpers' `setup_file` always loads the mock and fails
  fast if PATH does not include the stub directory.

- **[Trap-equivalent cleanup in bats]** `run.sh` uses `trap` for
  cleanup; bats uses `teardown`. The fixture written under
  `profiles/$INJ_PROFILE_NAME` (lines 177–219 of `run.sh`) writes into
  the *live* profiles tree, not into `$TMP`. Bats `teardown` runs even
  on test failure, but if the test process is killed (Ctrl-C), the
  fixture leaks. → Mitigation: rewrite the injection-allowlist test to
  use a temp dir under `$BATS_TEST_TMPDIR` with a stub `ROOT`
  override, eliminating the live-tree fixture entirely. This is the
  one place migration improves on `run.sh` rather than preserving it.

- **[Contributor friction]** Local test runs now require `bats-core`
  installed. → Mitigation: install steps in `CONTRIBUTING.md`; the CI
  workflow already does the install, so a contributor who runs `gh
  workflow run` instead can sidestep the local install.

- **[Bats-version drift between local and CI]** Different bats-core
  versions can have subtly different `run`/`status`/`output`
  semantics. → Mitigation: pin
  `bats-core/bats-action@3.0.0` in CI; document a minimum local
  version in `CONTRIBUTING.md`.

## Migration Plan

The migration is structured so CI is *always* green:

1. Add bats-core install + bats invocation to CI **alongside** the
   existing `bash scripts/tests/run.sh` step. CI runs both. (Initial
   bats suite is empty; the step is a no-op pass.)
2. Land the helpers (`setup.bash`, `mock-code-cli.bash`,
   `assertions.bash`).
3. Migrate each concern in its own commit (or PR slice if the project
   prefers): `validate-json.bats`, then `compose-settings.bats`, etc.
4. Parity-checkpoint task: produce a markdown table mapping every
   numbered assertion 1–14 in the spec's "Coverage parity" requirement
   to a bats `file:test-name`. The table lives in the change directory
   (`openspec/changes/migrate-tests-to-bats/parity.md`) and is reviewed
   by a human before deletion.
5. Delete `scripts/tests/run.sh` and remove its CI step in the same
   commit.
6. Update `README.md` and `CONTRIBUTING.md`.

**Bounded window**: the side-by-side state ends within three PRs of
the helpers PR merging — helpers PR, all-tests PR, parity-and-delete
PR. If the migration cannot fit that envelope it pauses for a
re-scoping discussion rather than letting two test surfaces coexist
indefinitely.

**Rollback**: if the bats suite proves problematic post-merge, revert
the deletion commit. The earlier commits (helpers + per-concern
migrations) are net additions and don't need to roll back.

## Open Questions

- *Should bats run with `--jobs N` in CI?* Defer to a follow-up;
  parallel-safety is built in but verifying it across all 9 files takes
  one extra CI cycle. Land sequential first.
- *Pin bats version locally?* `CONTRIBUTING.md` will document a
  minimum version. Strict pinning (e.g., `.bats-version` file) seems
  premature.
