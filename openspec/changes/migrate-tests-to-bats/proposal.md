## Why

`scripts/tests/run.sh` is a 294-line hand-rolled test runner that covers
fourteen distinct assertions (enumerated in the test-suite spec): composer path-containment (5 sub-cases),
extension-id allowlist on three call paths, PROFILE_ID generator fallback,
symlink-leaf invariant, workspace-trust default, mocked `code` CLI logging,
cache-skip behavior, and group-filtered installs. The runner provides no
test isolation (one shared `$TMP`, all tests share global env), no
per-test reporting (failure prints a single line and exits, hiding which
later tests would also have failed), no parallel execution, and no
structured output for CI to parse. It also bundles fixture builders, mocks,
assertion helpers, and the runner control flow into one file — anyone
adding a test today must read all 294 lines first.

Bats-core is the canonical bash test framework: TAP output for CI, per-test
isolation via `$BATS_TEST_TMPDIR`, parallel execution, dedicated mocking
patterns, and a `setup`/`teardown` lifecycle that mirrors xUnit. Migrating
now is cheap (12 test cases) and unblocks every future test — including
the regression test for `profile-graceful-defaults` (separate change) — to
be written in a framework rather than wedged into a growing monolith.

## What Changes

- Add bats-core as a project test dependency. Vendor or document install
  (Homebrew/apt/npm) and add a CI step.
- Create `scripts/tests/bats/` with one `.bats` file per logical concern:
  - `validate-json.bats` — JSON/JSONC validation pass.
  - `compose-settings.bats` — compose runs and produces symlinks; merge
    semantics; `@extends` path-containment cases (traversal, absolute,
    home-prefixed, malformed value, cycle through symlink).
  - `export-profiles.bats` — exports produce non-empty bundles.
  - `open-profiles.bats` — open invokes mocked `code` once per profile;
    cache-skip behavior on second invocation.
  - `install-extensions.bats` — group filter (`--group AI`) installs only
    AI-classified extensions.
  - `extension-id-allowlist.bats` — injection-shaped IDs rejected by all
    three call paths (`install-extensions.sh`, `import-profile.sh`,
    `vspcli --install-ext`).
  - `import-profile.bats` — PROFILE_ID generator: openssl→python3
    fallback when openssl is broken; safe under `set -euo pipefail`.
  - `symlink-invariants.bats` — `<stack>-{crisp,retina}.jsonc` leaves
    remain symlinks to `<stack>-base.jsonc`; spec-regex literal present
    verbatim in capability spec docs.
  - `workspace-trust.bats` — every merged profile has
    `security.workspace.trust.untrustedFiles = "prompt"`.
- Create `scripts/tests/bats/helpers/`:
  - `setup.bash` — shared `setup_file` for ROOT detection and PATH-stub
    bootstrap; per-test `setup`/`teardown` helpers.
  - `mock-code-cli.bash` — extracted `code` CLI stub (current lines
    15–47 of `run.sh`) reusable across tests.
  - `assertions.bash` — `assert_compose_rejects` and other shared
    assertion helpers.
- Update `.github/workflows/ci.yml`: replace
  `bash scripts/tests/run.sh` with a `bats scripts/tests/bats/` step,
  preceded by a bats-core install step (use `bats-core/bats-action` or
  `npm install -g bats`).
- Delete `scripts/tests/run.sh` after the bats suite is at parity. The
  deletion is the final task, not the first — keep both running side
  by side during migration.
- **BREAKING for contributors**: developers running tests locally must
  install `bats-core`. Document in `README.md` and `CONTRIBUTING.md`.

## Capabilities

### New Capabilities

- `test-suite`: defines the project's test framework choice
  (bats-core), test layout under `scripts/tests/bats/`, helper
  conventions (`setup.bash`, `mock-code-cli.bash`, `assertions.bash`),
  CI invocation, and the parity guarantee that every assertion in the
  legacy `scripts/tests/run.sh` has an equivalent bats test before the
  legacy runner is deleted.

### Modified Capabilities

(none — the legacy runner has no spec; this change introduces the
first spec for testing.)

## Impact

- **Code**: `scripts/tests/bats/**` (new), `scripts/tests/run.sh` (deleted),
  `.github/workflows/ci.yml` (CI invocation), `README.md`,
  `CONTRIBUTING.md`.
- **Dependencies**: new — `bats-core` (required to run tests locally and
  in CI). No runtime/production dependency added; tests-only.
- **APIs**: none.
- **Backwards compatibility**: contributor-facing only. CI step changes;
  local test invocation changes from `bash scripts/tests/run.sh` to
  `bats scripts/tests/bats/`. No behavior of any production script is
  modified.
- **Risk**: medium. The current suite contains fiddly assertions
  (PROFILE_ID hex shape, mocked-CLI invocation counts, fixture-tree
  builds, extension-id injection across three call paths) that must be
  preserved exactly. A faithful migration is more work than a clean-slate
  rewrite would be — and a clean-slate rewrite would lose coverage
  silently. Mitigation: the parity-checkpoint task requires every
  current `run.sh` assertion to have a corresponding bats test before
  `run.sh` is deleted.
