## 1. Security defaults

- [x] 1.1 Set `security.workspace.trust.untrustedFiles` to `"prompt"` in `_shared/editor-crisp.jsonc` and `_shared/editor-retina.jsonc`
- [x] 1.2 Restore `editor.fontVariations` to `true` in `_shared/editor-{crisp,retina}.jsonc` (regression confirmed in original review)
- [x] 1.3 Replace the `security@example.com` placeholder in `SECURITY.md` with a real contact (or remove the section)
- [x] 1.4 Add a Migration / Behavior Change section to `README.md` documenting the Workspace Trust prompt that users will see on next folder open

## 2. Atomic compose pipeline

- [x] 2.1 In `scripts/compose-settings.sh`, write the jq-merged output to a sibling temp path (`<target>.tmp.$$`) and `mv -f` to the final path; use `mktemp` only with explicit templates that work on both macOS and GNU
- [x] 2.2 Remove `2>/dev/null || true` from the `@extends` parse step; surface jq errors and abort the merge for that profile
- [x] 2.3 Accept only bare filenames matching `^[a-zA-Z0-9._-]+(/[a-zA-Z0-9._-]+)*\.jsonc$` (allows subdir refs like `ai/copilot.jsonc`) resolving inside `_overrides/`; reject `..` segments, leading `/`, leading `~`, backslashes, NUL bytes; containment guard re-checks via realpath
- [x] 2.4 Switch the cycle detector to compare resolved real paths (`realpath` with python fallback) so cycles through symlinks are caught
- [x] 2.5 Add a regression test in `scripts/tests/run.sh` that creates a fixture mini-repo and exercises 5 cases: traversal, absolute, home-prefixed, malformed @extends value, cycle through symlink — asserts compose fails loudly with a recognizable error string for each

## 3. Extension ID allowlist

- [x] 3.1 Create `scripts/lib/extension-id.sh` exporting `EXTENSION_ID_REGEX='^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$'` and a `validate_extension_id` function that prints the offending value on rejection and returns non-zero
- [x] 3.2 Source the helper in `scripts/install-extensions.sh` and validate every ID before invoking `code --install-extension`; reject with non-zero exit on a violation
- [x] 3.3 Source the helper in `scripts/import-profile.sh`; remove the trailing `|| true` from the install loop so failures surface; collect failed IDs and exit non-zero after the loop completes
- [x] 3.4 Source the helper in `scripts/vspcli` for `--install-ext`
- [x] 3.5 Add a test in `scripts/tests/run.sh` that exercises ALL THREE paths (install-extensions.sh, import-profile.sh via a malformed `.code-profile`, vspcli --install-ext) with an injection-shaped value and asserts each path rejects it
- [x] 3.6 Add a code-spec drift assertion to the test harness — extracts the regex literal from the helper and `grep -F`s it in each of the three spec deltas (install-profile-extensions, import-profile-bundles, manage-profile-cli)

## 4. Robust import-profile.sh

- [x] 4.1 Replace the `tr -dc 'a-f0-9' < /dev/urandom | head -c 8` PROFILE_ID with `openssl rand -hex 4` (with python3 `secrets.token_hex(4)` fallback when openssl is absent)
- [x] 4.2 Add a test exercising the import path under `set -euo pipefail` to assert no SIGPIPE-induced abort

## 5. Fix check-extension-compatibility.sh

- [x] 5.1 Replace the bogus `code --show-extension` call with `code --list-extensions --show-versions --profile <name>` and parse the version
- [x] 5.2 Wrap each per-extension compat call as `if compat_info=$(check_compatibility "$ext_id"); then rc=0; else rc=$?; fi` so the helper's 1/2 returns are captured without `set -e` aborting the loop
- [x] 5.3 Pass `--profile <name>` to `code --list-extensions` lookups so installed-version checks are profile-scoped
- [x] 5.4 Make the `usage()` error path `exit 2` for CLI misuse only; helper internal returns of `1`/`2` never propagate (process exit is `0` clean / `1` findings / `2` misuse)
- [x] 5.5 Document the exit code contract in the script header

## 6. Surface failures elsewhere

- [x] 6.1 In `scripts/open-profiles.sh`, replace `code --profile "$p" --new-window || true` with explicit failure capture; collect failed profiles and print them; exit non-zero if any failed
- [x] 6.2 In `scripts/validate-json.sh`, default `TMPDIR` with `: "${TMPDIR:=/tmp}"` before any `set -u` reference

## 7. Repository hygiene

- [x] 7.1 Add `.cache/` to `.gitignore`
- [x] 7.2 `git rm -r --cached .cache/`
- [x] 7.3 Confirm `.gitignore` covers any other generated machine-state paths surfaced by `git status` after a clean compose+export run

## 8. CI workflow

- [x] 8.1 Add `.github/workflows/ci.yml` pinned to `runs-on: ubuntu-latest`. Step order: install jq + openssl; run `scripts/compose-settings.sh && scripts/export-profiles.sh`; assert `git diff --exit-code _merged/ exports/`; run `scripts/validate-json.sh`; run `scripts/tests/run.sh`.
- [x] 8.2 Add `scripts/install-hooks.sh` — an idempotent installer that reads `git config --get core.hooksPath` and refuses to overwrite a foreign value. Referenced from `CONTRIBUTING.md`.
- [x] 8.3 Add a workflow status badge to `README.md`
- [x] 8.4 Add a `tests/run.sh` assertion that `_overrides/java-spring-{crisp,retina}.jsonc` are symlinks pointing at `java-spring-base.jsonc`
- [x] 8.5 Add a constraint to `openspec/project.md` documenting the symlink trick used for Java Spring DPI variants

## 9. Final regenerate (do LAST after all edits)

- [x] 9.1 Run `scripts/compose-settings.sh` to regenerate every `_merged/*.json` from the corrected sources
- [x] 9.2 Run `scripts/export-profiles.sh` to regenerate every `exports/*.code-profile`
- [x] 9.3 `git status` confirms only the expected files: shared bases, all _merged + exports, all scripts, CHANGELOG/CI/lib/install-hooks new files, .cache deletions, openspec change folder. No stray drift.

## 10. Validation

- [x] 10.1 Run `openspec validate harden-profile-tooling-and-pipeline --strict` — passes
- [x] 10.2 Run `scripts/tests/run.sh` locally and confirm all 11 test groups pass
- [x] 10.3 Compose+export are idempotent (verified by md5-hashing all artifacts before and after a second regen). CI's `git diff --exit-code _merged/ exports/` gate will pass once the diff is committed.
- [x] 10.4 Code-spec drift confirmed by `tests/run.sh` group "extension-id regex literal in helper matches the spec deltas"
