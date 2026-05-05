## 1. CI bootstrap (both runners green + linting)

- [ ] 1.1 Add `bats-core/bats-action@3.0.0` install step to `.github/workflows/ci.yml` ahead of the existing `bash scripts/tests/run.sh` step.
- [ ] 1.2 Add `bats scripts/tests/bats/` invocation step after the install. Suite is empty at this point — the step is a no-op pass and proves the CI integration works.
- [ ] 1.3 Add a `shellcheck` CI step that lints every `*.sh` under `scripts/` plus every `*.bash` helper under `scripts/tests/bats/helpers/`. Use `ludeeus/action-shellcheck@master` or equivalent. Configure to pass `--severity=warning` initially; tighten to `--severity=style` post-migration. The shellcheck step is a permanent addition (does not get removed alongside `run.sh` in §5).
- [ ] 1.4 Verify CI is green on a draft PR: `run.sh`, empty bats step, and shellcheck all pass.

## 2. Test layout and helpers

- [ ] 2.1 Create `scripts/tests/bats/` and `scripts/tests/bats/helpers/`.
- [ ] 2.2 Write `helpers/setup.bash`: exports `ROOT`, sets `VSCODE_EXTENSION_INSTALL_DELAY=0`, defines a per-test `make_tmp_root` helper for tests that need a sibling `scripts/_shared/_overrides/profiles` fixture tree under `$BATS_TEST_TMPDIR`. **Shell hygiene**: every helper file starts with `# shellcheck shell=bash` (the file is sourced, not run, so shellcheck cannot infer dialect from a shebang). Bats helpers do not get a shebang.
- [ ] 2.3 Write `helpers/mock-code-cli.bash`: extracts the `code` CLI stub from `run.sh:15-47` into a reusable function `install_mock_code_cli` that takes a target dir, writes the stub, prepends to PATH, sets `CODE_LOG`. Add a fail-fast check that PATH starts with the mock dir. Same `# shellcheck shell=bash` directive at top.
- [ ] 2.4 Write `helpers/assertions.bash`: extract `assert_compose_rejects` from `run.sh:123-132` and any other shared assertions discovered during migration. Same `# shellcheck shell=bash` directive at top.
- [ ] 2.5 Pin a minimum bats version in `CONTRIBUTING.md` (target the version installed by `bats-core/bats-action@3.0.0`).
- [ ] 2.6 Confirm all helper files clear `shellcheck` (the §1.3 CI step covers this on PR; verify locally first via `shellcheck scripts/tests/bats/helpers/*.bash`).

## 3. Migrate per-concern (each task = one `.bats` file at parity)

For each `.bats` file: write the file, run `bats <file>` locally, confirm it passes; then on CI, confirm both `run.sh` (which still has the same assertion) and the new bats test pass for the same logical case.

- [ ] 3.1 `scripts/tests/bats/validate-json.bats` — covers `run.sh:56-57` (assertion #1).
- [ ] 3.2 `scripts/tests/bats/compose-settings.bats` — covers `run.sh:59-63, 105-165` (assertions #2, #3, #8 with all 5 sub-cases). Use `make_tmp_root` for the `@extends` containment fixtures.
- [ ] 3.3 `scripts/tests/bats/export-profiles.bats` — covers `run.sh:65-69` (assertion #4).
- [ ] 3.4 `scripts/tests/bats/open-profiles.bats` — covers `run.sh:71-83, 94-103` (assertions #5, #6). Loads `mock-code-cli.bash`.
- [ ] 3.5 `scripts/tests/bats/install-extensions.bats` — covers `run.sh:85-92` (assertion #7). Loads `mock-code-cli.bash`.
- [ ] 3.6 `scripts/tests/bats/extension-id-allowlist.bats` — covers `run.sh:167-219` (assertion #9) across all three call paths (`install-extensions.sh`, `import-profile.sh`, `vspcli --install-ext`). **Isolation pattern**: production scripts use a fixed `ROOT="$(cd "$(dirname "$0")/.." && pwd)"` and have no env override. Use the existing `cp` pattern from `run.sh:114` — copy each script into `$BATS_TEST_TMPDIR/repo/scripts/` and build a minimal sibling tree (`scripts/lib/extension-id.sh`, `profiles/<fixture>/extensions.json`). This eliminates the `INJ_PROFILE_DIR` write into the live `profiles/` tree (`run.sh:177-181`) without modifying any production script (preserves the design.md non-goal "Rewriting the production scripts under test"). Verify post-test that `git status --porcelain` shows no changes.
- [ ] 3.7 `scripts/tests/bats/import-profile.bats` — covers `run.sh:221-255` (assertions #10, #11). Stubs a broken `openssl` ahead of the real one; verifies python3 fallback and `set -euo pipefail` safety.
- [ ] 3.8 `scripts/tests/bats/symlink-invariants.bats` — covers `run.sh:257-279` (assertions #12, #13). Pure repo-state assertions, no fixtures needed. **Pre-write check for #12**: `harden-profile-tooling-and-pipeline` may have been archived between proposal and implementation. Resolve each capability spec via the spec's documented order: try `openspec/specs/<cap>/spec.md` first; on miss fall back to `openspec/changes/harden-profile-tooling-and-pipeline/specs/<cap>/spec.md`. Implement this fallback in the bats test itself (not in a one-shot path lookup) so future archives don't break it.
- [ ] 3.9 `scripts/tests/bats/workspace-trust.bats` — covers `run.sh:281-291` (assertion #14). Runs `compose-settings.sh` first to populate `_merged/`, then asserts.

## 4. Parity checkpoint (mechanized + manual)

- [ ] 4.1 Produce `openspec/changes/migrate-tests-to-bats/parity.md`: a table with one row per numbered assertion 1–14 from `specs/test-suite/spec.md` "Coverage parity" requirement, mapping each to `scripts/tests/bats/<file>:<test-name>`. Cross-check against `run.sh` line numbers.
- [ ] 4.2 Write the parity verifier per the spec's "Mechanized parity verification" requirement. **Format constraint**: `parity.md` MUST use a fixed-shape line format (e.g., one assertion per line, `^N\. <file>:<test-name>$`) so the verifier can parse with a single regex and not reinvent markdown-table parsing. **Implementation choice**: prefer `scripts/tests/check-parity.sh` (bash) only if it stays under the shell-authoring 100-line threshold; otherwise write `scripts/tests/check-parity.py` (Python 3, stdlib only — no new dependency). Either way: must exit non-zero on any miss, must obey `#!/usr/bin/env bash` or `#!/usr/bin/env python3` shebang, must clear `shellcheck`/`ruff` respectively. Add a CI step that runs this verifier ahead of `bats scripts/tests/bats/`. (Removed in §5 when `run.sh` is deleted.)
- [ ] 4.3 Manual review: a maintainer confirms the table semantically captures every `run.sh` assertion (the verifier proves *file+name resolve*; only a human can confirm the *test logic* matches the original). This is the gate for §5.
- [ ] 4.4 Verify bats suite passes under `--jobs 4` (parallel-safety check) at least once locally; do not enable in CI yet.

## 4b. Timing constraint

- [ ] 4b.1 The side-by-side period (where both `run.sh` and bats run in CI) MUST end within three PRs after the helpers PR (§2) merges: helpers PR, all-tests PR, parity-and-delete PR. If the migration cannot fit that envelope, pause and discuss before continuing — drift is the failure mode this constraint exists to prevent.

## 5. Cut over

- [ ] 5.1 Remove the `bash scripts/tests/run.sh` step from `.github/workflows/ci.yml`.
- [ ] 5.2 Remove the parity-verifier invocation step from `.github/workflows/ci.yml` and delete `scripts/tests/check-parity.sh` (or `check-parity.py`, whichever was chosen in §4.2). The verifier was a migration aid; with `run.sh` gone there is no parity to verify. **Note**: the shellcheck CI step from §1.3 is permanent and stays.
- [ ] 5.3 Delete `scripts/tests/run.sh`. Same commit as 5.1 and 5.2.
- [ ] 5.4 Verify CI is green on a fresh PR with bats-only.

## 6. Documentation

- [ ] 6.1 `README.md`: update any "Run tests with …" line to `bats scripts/tests/bats/`.
- [ ] 6.2 `CONTRIBUTING.md`: add a "Running tests" section with bats-core install steps for macOS (`brew install bats-core`), Debian/Ubuntu (`apt install bats`), and npm (`npm install -g bats`). Mention the minimum version pinned in §2.5. Also add a "Shell scripts" section noting: `set -euo pipefail` + `IFS=$'\n\t'` preamble; `[[ … ]]` over `[ … ]`; cite `shellcheck` SC-codes (link pattern `https://www.shellcheck.net/wiki/SC<N>`) in review comments; helper files use `# shellcheck shell=bash` directive in lieu of shebang.
- [ ] 6.3 `agents/` docs: if any LLM-facing doc references `scripts/tests/run.sh`, update to `scripts/tests/bats/`.

## 7. Validate the change

- [ ] 7.1 Run `openspec validate migrate-tests-to-bats --strict` and resolve any findings.
- [ ] 7.2 Open the PR; ensure the parity table at §4.1 is linked in the description.
