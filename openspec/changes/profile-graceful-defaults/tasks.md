## 1. Build the label-to-extension-ID map

- [x] 1.1 Implement the label-to-extension-ID map in `scripts/import-profile.sh` as a `case` statement (NOT a Bash associative array). Reason: macOS still ships Bash 3.2 as `/bin/bash`; associative arrays require 4+. While the script's shebang is `#!/usr/bin/env bash` (so it picks up Homebrew Bash 5 on a developer machine), CI workers, fresh checkouts, and the existing test fixtures may not. A `case` statement is 3.2-safe and reads cleanly. Cover every theme/icon-theme referenced by the project's profile bundles; cross-check against `_shared/editor-{crisp,retina}.jsonc` and `_overrides/ai-*`, and confirm the entries match the extension IDs in `exports/*.code-profile`.
- [x] 1.2 Add a Bash function `resolve_theme_to_ext_id(value)` that prints the resolved `<publisher>.<name>` to stdout if the input is either an explicit extension ID or a known label in the §1.1 case statement, and prints nothing (empty string) otherwise. **Reuse the existing `EXTENSION_ID_REGEX` from `scripts/lib/extension-id.sh:8`** (already sourced by `import-profile.sh:6`); do NOT introduce a new regex. The check is `[[ "$value" =~ $EXTENSION_ID_REGEX ]]`. This keeps the explicit-ID rule aligned with the install-path safety constraints already enforced by `validate_extension_id` and avoids regex drift between two places. Quote all expansions per shellcheck SC2086. Use `[[ ... ]]` not `[ ... ]` per skill rule 5.

## 2. Wire the rewrite pass into the install loop

- [x] 2.0 **Migrate existing failure output to stderr** (precondition for §2.2's stderr convention). The current `import-profile.sh:144-148` final failure summary uses bare `printf` and goes to stdout, while the per-extension warning at line 138 uses `>&2` (stderr) — inconsistent. Redirect the two `printf` calls at lines 145–146 to stderr by appending `>&2` to each. This brings the script in line with the shell-authoring convention (rule 7: errors and informational notices to stderr) and makes the stderr-based theme-strip notices added in §2.2 consistent with the script's other failure output.
- [x] 2.2 After the loop ends, before the existing `if [ "${#FAILED_EXT[@]}" -gt 0 ]` block, add a new function `rewrite_settings_for_failed_themes()` that:
  - Reads `$TARGET_DIR/settings.json` (the imported profile cache).
  - For each of `workbench.colorTheme` and `workbench.iconTheme`: extract the value, call `resolve_theme_to_ext_id`, check membership in the failed-install list. **Conventions** (skill rules 5, 6, 10, plus existing repo idioms):
    - Use `[[ ... ]]` not `[ ... ]` for tests.
    - Split `local`-with-substitution declarations: write `local id; id=$(resolve_theme_to_ext_id "$value")`, never `local id=$(...)` — the latter is SC2155 (swallows exit code).
    - Gate iteration on length, matching the existing pattern at lines 132–144: `if [ "${#FAILED_EXT[@]}" -gt 0 ]; then for f in "${FAILED_EXT[@]}"; do ...`. Don't dereference `"${FAILED_EXT[@]}"` outside that guard — under bash 3.2 + `set -u` the empty-array dereference aborts.
    - Quote every expansion: `"${value}"`, `"$(jq ... "${file}")"`.
  - If any key was marked, write a single `jq` invocation that deletes only the keys in question (build the `del(.[...])` chain dynamically based on the marked-list). Use the **atomic-write pattern from `compose-settings.sh:144-150`**: a sibling temp file in the same directory (`local out_tmp="$TARGET_DIR/settings.json.tmp.$$"`), then `mv -f "$out_tmp" "$TARGET_DIR/settings.json"`. Sibling-fs is required for atomic mv across fs boundaries on macOS APFS.
  - Print to **stderr** (skill rule 7 reserves stderr for non-success output) for each key removed: `"import-profile: removed workbench.colorTheme (extension <id> failed to install)" >&2`. After §2.0 lands, the existing final failure summary at `import-profile.sh:144-148` will also be on stderr — the new theme-strip lines and the failure summary share the same stream.
- [x] 2.3 Call `rewrite_settings_for_failed_themes` from the main flow only when `FAILED_EXT[@]` is non-empty. No-op when the list is empty (Decision 1, no work happens on the happy path; also avoids the bash-3.2 unbound-array footgun).
- [x] 2.4 Verify shellcheck passes on the modified `scripts/import-profile.sh` with no new findings (cite SC-codes if any are intentionally disabled). Run it against a fixture bundle locally and confirm the rewrite happens for a simulated failure (use `VSCODE_USER_DIR=$(mktemp -d)` to avoid touching the live VS Code cache).

## 3. Surface the canonical import path

- [x] 3.1 Append a notice to the end of `scripts/export-profiles.sh` directing recipients at `scripts/import-profile.sh`: `printf '%s\n' 'Import bundles via: scripts/import-profile.sh <name> exports/<name>.code-profile' 'NOTE: the VS Code UI import does NOT install missing extensions and may produce a degraded look.'`. **stdout, not stderr**: matches the existing per-profile success messages on `export-profiles.sh:52` ("Exported $name -> ...") and `compose-settings.sh:155` ("Merged $name -> ..."), which both go to stdout. The notice is a doc footer, conceptually similar to those success lines. Errors and skip-reasons in those scripts go to stderr (e.g., `export-profiles.sh:5,16,26,34,38`); progress and success go to stdout. Match that pattern.
- [x] 3.2 Edit `README.md` Quick Start so `scripts/import-profile.sh` is the documented import command, with a one-line warning that VS Code's UI import (Settings → Profiles → Import) does NOT install missing extensions and may produce a degraded look. The warning should be visible at the same heading depth as the script command, not buried in a footnote.

## 4. Regression tests

Writing tests follows `artagon-shell:shell-testing` and `wshobson-shell-scripting--bats-testing-patterns`: test the CLI contract (exit code + stdout/stderr + filesystem state), use per-test temp dirs, mock external deps, assert specific exit codes (`-eq N` not `-ne 0`), one assertion per test, descriptive test names.

- [ ] 4.0 **Test infrastructure**:
  - **Skip gates**: each test file's `setup_file` (bats) or `setup` (run.sh) checks for `jq` and `code` — if either is missing, `skip "jq/code required"`. Mirrors the wshobson skill's "Testing with Dependencies" pattern.
  - **Fixture directory**: create `scripts/tests/fixtures/import-profile/` (or `scripts/tests/bats/fixtures/import-profile/` for the bats path) with named `.code-profile` files: `with-tokyo-night.code-profile`, `with-tokyo-night-and-iconv.code-profile`, `with-builtin-theme.code-profile`, `with-explicit-id-theme.code-profile`. Tests reference these by name rather than building bundles inline via heredoc.
  - **Parallel-safety**: `FAILING_EXTS` is a process env var; if bats runs `--jobs N` two tests using different `FAILING_EXTS` values can race the mocked `code`'s reads. Tag the bats file with `# bats file_tags=:no-parallel` (bats 1.10+) to force sequential execution within this file. Other bats files in the suite remain parallelizable. Document this constraint at the top of the file.
  - **Teardown env hygiene**: each test's `teardown` MUST `unset FAILING_EXTS VSCODE_USER_DIR` to prevent leakage into the next test. Add this to `helpers/setup.bash` if a shared teardown is more ergonomic.

- [ ] 4.1 Choose the test surface: if PR #3 (`migrate-tests-to-bats`) has merged at the time of implementation, write tests as `scripts/tests/bats/import-profile.bats`. Otherwise add to `scripts/tests/run.sh`. Document the choice as the first line of the test file/section.

- [ ] 4.2 Mock contract for the `code` CLI stub. Both test surfaces need a `code` mock that fails specific extension installs.
  - **Bats path**: extend `helpers/mock-code-cli.bash` (from PR #3) to support a `FAILING_EXTS` env var: a comma- or newline-separated list of extension IDs. When `code --install-extension <id>` is called for an `<id>` in that list, the stub exits 1 and writes a `action=install_failed profile=<n> ext=<id> exit=1` log line. Otherwise behaves as today.
  - **run.sh path**: extend the inline `code` stub at `run.sh:15-47` with the same `FAILING_EXTS` semantics.

- [ ] 4.3 Per-test temp isolation:
  - **Bats path**: every test uses `$BATS_TEST_TMPDIR` for `VSCODE_USER_DIR` and bundle copies. Bats auto-cleans this per-test.
  - **run.sh path**: each new assertion gets its own `local sandbox; sandbox=$(mktemp -d -p "$TMP" import-profile-XXXX)` — subdirectory under the existing run-wide `$TMP`. Parallel-unsafe but matches existing run.sh style; the bats migration fixes parallel-safety later.

- [ ] 4.4 **Test: `import-profile.sh strips workbench.colorTheme when its extension fails to install`** (the core regression).
  - Fixture: a `.code-profile` bundle in `<sandbox>/bundle.code-profile` with `settings = { "workbench.colorTheme": "Tokyo Night" }` and `extensions.enabled = ["enkia.tokyo-night", "github.copilot"]` (one theme, one decoy).
  - Setup: `FAILING_EXTS=enkia.tokyo-night`.
  - Run: `VSCODE_USER_DIR=<sandbox>/vscode FAILING_EXTS=enkia.tokyo-night bash scripts/import-profile.sh test-profile <sandbox>/bundle.code-profile`.
  - Assert (all three):
    1. `[ "$status" -eq 1 ]` (the script exits 1 when any install failed; this is the existing contract from `import-profile.sh:148`).
    2. The post-import `settings.json` (`<sandbox>/vscode/profiles/<id>/settings.json`) does NOT contain a `workbench.colorTheme` key. Verify with `jq -e 'has("workbench.colorTheme")' <file>` returning false.
    3. Stderr (`$output` in bats `run`, or captured separately in run.sh) contains the substring `"removed workbench.colorTheme (extension enkia.tokyo-night failed to install)"`.

- [ ] 4.5 **Test: `import-profile.sh strips workbench.iconTheme but preserves workbench.colorTheme when only the icon-theme extension fails`**.
  - Fixture: `with-tokyo-night-and-iconv.code-profile` — `settings = { "workbench.colorTheme": "Tokyo Night", "workbench.iconTheme": "material-icon-theme" }` and `extensions.enabled = ["enkia.tokyo-night", "pkief.material-icon-theme"]`.
  - Setup: `FAILING_EXTS=pkief.material-icon-theme`.
  - Assert: `workbench.iconTheme` is absent from the post-import settings; `workbench.colorTheme` is preserved.

- [ ] 4.6 **Test: `import-profile.sh preserves a built-in theme name when an unrelated extension fails`** (covers the "Theme value is unrecognised" spec scenario).
  - Fixture: `with-builtin-theme.code-profile` — `settings = { "workbench.colorTheme": "Default Dark Modern" }` and `extensions.enabled = ["github.copilot"]`.
  - Setup: `FAILING_EXTS=github.copilot`.
  - Assert: `workbench.colorTheme` is preserved (value not in the resolution map; presumed built-in). Script exits 1 because the install failed. No "removed workbench.colorTheme" line on stderr.

- [ ] 4.7 **Test: `import-profile.sh produces a byte-identical settings.json on the happy path`** (the inverse — guards against the rewrite firing when no extensions failed).
  - Fixture: `with-tokyo-night.code-profile`. `FAILING_EXTS=` empty.
  - Assert: post-import `<vscode>/profiles/<id>/settings.json` is byte-equal to the bundle's `settings` block (extracted via `jq '.settings'` and compared). `[ "$status" -eq 0 ]`. No "removed" stderr lines.

- [ ] 4.8 **Test: `import-profile.sh strips a theme key whose value is an explicit publisher.name extension ID`** (covers spec rule 1).
  - Fixture: `with-explicit-id-theme.code-profile` — `settings = { "workbench.colorTheme": "enkia.tokyo-night" }`.
  - Setup: `FAILING_EXTS=enkia.tokyo-night`.
  - Assert: key is stripped. This proves the explicit-ID form took the "rule 1" path in `resolve_theme_to_ext_id` (no label-table lookup needed).

## 5. Shell-authoring conventions (all script edits)

Apply across §§1–3. These mirror the `artagon-shell:shell-authoring` skill and the existing repo conventions; non-compliance fails review:

- [ ] 5.1 Shebang stays `#!/usr/bin/env bash` (skill rule 1). Don't switch to `#!/bin/bash`.
- [ ] 5.2 Strict-mode preamble `set -euo pipefail` is already in `import-profile.sh:2` and `export-profiles.sh:2` — keep it. Do NOT add `IFS=$'\n\t'` mid-script; the existing scripts don't set it and changing IFS now would silently re-shape word-splitting elsewhere.
- [ ] 5.3 Every variable expansion gets braces and quotes: `"${var}"` not `$var` (SC2086).
- [ ] 5.4 Tests use `[[ ... ]]` not `[ ... ]` (skill rule 5; SC2007/SC2027 family). One exception: when extending an `if [` block whose siblings already use single-bracket `[`, match local style for diff hygiene.
- [ ] 5.5 `local x=$(cmd)` is forbidden (SC2155). Always split: `local x; x=$(cmd) || handle_error`.
- [ ] 5.6 Array iteration uses `"${arr[@]}"` (SC2068, SC2124). Empty-array dereference under `set -u` and bash 3.2 aborts; gate with `[ "${#arr[@]}" -gt 0 ]` first.
- [ ] 5.7 Errors go to stderr (`>&2`); progress/success/data goes to stdout. Match existing repo idioms: per-extension warnings at `import-profile.sh:138` use `>&2` ✓, `export-profiles.sh:5,16,26,34,38` errors use `>&2` ✓, success messages at `export-profiles.sh:52` and `compose-settings.sh:155` use stdout ✓. The current final-failure summary at `import-profile.sh:144-148` is on stdout — that's an inconsistency `§2.0` corrects by adding `>&2`.
- [ ] 5.8 `cd path || exit 1` for any new `cd` calls (skill rule 8). The current scripts use `cd ... && pwd` patterns at the top — match that style for ROOT detection.
- [ ] 5.9 Run `shellcheck scripts/import-profile.sh scripts/export-profiles.sh` after each change set; resolve all warnings or document the SC-code with `# shellcheck disable=SC<N>` and a one-line justification.

## 6. Validate the change

- [ ] 6.1 Run `openspec validate profile-graceful-defaults --strict` and resolve any findings.
- [ ] 6.2 Run `bash scripts/tests/run.sh` (or the bats suite, depending on §4.1 choice) and confirm all tests pass, including the new regression test.
- [ ] 6.3 Manual smoke test: build a current `.code-profile` from the project, run `import-profile.sh` against a throwaway `VSCODE_USER_DIR`, confirm the imported `settings.json` is byte-equal to the bundle's `settings` object (no spurious changes when nothing fails).
