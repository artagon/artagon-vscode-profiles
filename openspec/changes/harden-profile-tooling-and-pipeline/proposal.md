## Why

A three-reviewer adversarial review (Claude + Codex + Gemini, 2026-04-27) of the profile tooling found nine BLOCKER/HIGH issues and several MEDIUM issues spanning security, correctness, and supply-chain risk. A second adversarial pass on the original draft of this proposal (2026-04-28) caught a wrong-shape Spring fix, a missing spec contract for the highest-severity security finding, implementation-leaking spec scenarios, three silently-dropped pre-flagged findings, and several task-ordering and CI-coverage gaps. This revision incorporates those corrections.

Every shipped profile currently disables VS Code Workspace Trust, the compose pipeline writes merged settings non-atomically into a path that is symlinked into live profiles, the compatibility checker is broken in two ways at once, and extension IDs from arbitrary `.code-profile` bundles are piped straight into `code --install-extension` with no allowlist. The originally-flagged "Spring profile family extends the wrong base" finding turned out to be a false positive: `_overrides/java-spring-{crisp,retina}.jsonc` are git symlinks (mode `120000`) pointing at `java-spring-base.jsonc`, so the leaf identity is supplied by the filename while the content is shared — no duplication exists and the @extends chain is irrelevant. That finding has been retired.

These are pre-existing defects and do not depend on each other. Bundling them into one OpenSpec change keeps the audit trail coherent: each tasks-level item maps to a finding from `.agents/review-2026-04-27/logs/`.

## What Changes

- **BREAKING (security):** `_shared/editor-{crisp,retina}.jsonc` flip `security.workspace.trust.untrustedFiles` from `"open"` to `"prompt"`. Affects every shipped profile. Contracted as a new `secure-shared-defaults` capability so future regressions are caught at PR review.
- Restore `editor.fontVariations` to `true` in both shared bases (regression caught in the original review and silently dropped from the first proposal draft).
- Make `_merged/<name>.json` writes atomic in `scripts/compose-settings.sh` (write to temp, `mv -f`).
- Stop swallowing `jq` errors when parsing `@extends` in `compose-settings.sh`.
- Reject `@extends` paths containing `..`, leading `/`, leading `~`, backslashes, or NUL bytes so the chain cannot escape `_overrides/`. Cycle detection compares resolved real paths so cycles through symlinks are caught.
- Validate extension IDs against an allowlist regex in a single sourced helper at `scripts/lib/extension-id.sh`, used by `scripts/install-extensions.sh`, `scripts/import-profile.sh`, and `scripts/vspcli`. Stop hiding install errors with `|| true` in `import-profile.sh`.
- Replace the SIGPIPE-prone `tr | head` PROFILE_ID generator in `scripts/import-profile.sh` with a method that is safe under `set -euo pipefail`.
- Fix `scripts/check-extension-compatibility.sh`: use `code --show-versions`, capture per-extension exit codes without aborting under `set -e`, query installed extensions per `--profile`, and exit non-zero on misuse (reserve exit code 2 for CLI misuse, distinct from the helper's internal "unknown" return).
- Stop swallowing `code --new-window` failures in `scripts/open-profiles.sh`.
- Default `TMPDIR` in `scripts/validate-json.sh` so it runs under stripped environments.
- Add `.cache/` to `.gitignore` and stop tracking `.cache/extensions-installed/`.
- Add a CI workflow under `.github/workflows/` (Linux runner). Step order: regen → diff `_merged/` and `exports/` → validate-json → tests, so tests always see freshly composed artifacts and never pass against stale committed output.
- Add `scripts/install-hooks.sh` — an idempotent installer that refuses to clobber a foreign `core.hooksPath`, so users with husky/pre-commit/etc. are not surprised.
- Add `[[ -L … ]]` assertions in `scripts/tests/run.sh` and a Constraints note in `openspec/project.md` to protect the load-bearing symlink trick used for `_overrides/java-spring-{crisp,retina}.jsonc`.
- Replace SECURITY.md placeholder address.
- Add a release-notes / README migration entry for the Workspace Trust behavior change.

Out of scope (explicitly deferred to follow-up changes):
- Collapsing the crisp/retina axis (Gemini M2 — architecture refactor, large surface area)
- De-duplicating `Continue.continue` / `continue.continue` extension entries (Gemini M1 — data-only, isolated)
- Removing `_merged/` and `exports/` from version control (workflow change requiring CI prerequisite from this proposal)
- Renaming `_overrides/*.jsonc` to `*.json` to match their actual strict-JSON content (Gemini H3 — touches every override file and CONTRIBUTING.md; defer until after the helper consolidation in §4 lands)
- Word-splitting hardening inside `check-extension-compatibility.sh` JSON assembly (Gemini S3 — bounded by §6 audit fixes; will be re-evaluated post-§6)
- Pinning extension versions in `extensions.json` (Gemini SC1 — requires marketplace API integration, separate proposal)

## Impact

- Affected specs:
  - **`secure-shared-defaults` (NEW)** — contracts the Workspace Trust default and the variable-font policy for shared bases
  - **`audit-profile-extensions` (NEW)** — contracts the previously-unspecified `check-extension-compatibility.sh` capability
  - `compose-profile-settings` — atomic write requirement; strict `@extends` parse error handling; path containment for `@extends`
  - `install-profile-extensions` — extension ID allowlist requirement
  - `import-profile-bundles` — extension ID allowlist; ID generator robustness; surfaced install errors
  - `open-profiles` — surfaced launch failures
  - `manage-profile-cli` — extension ID allowlist when delegating
  - `validate-profile-json` — TMPDIR default
- Affected code: `_shared/editor-{crisp,retina}.jsonc`, all of `scripts/`, new `scripts/lib/extension-id.sh`, `.gitignore`, `.github/workflows/`, `SECURITY.md`, `README.md`.
- User-visible change: existing users will see a Workspace Trust prompt the first time they open an untrusted folder under a managed profile. This is the intended secure default; documented in README.
- Reviews referenced: `.agents/review-2026-04-27/logs/code-review/codex-adversarial.jsonl`, `.agents/review-2026-04-27/logs/code-review/gemini-architecture.md`, `.agents/review-2026-04-27/logs/proposal-review/codex-adversarial.md` (Codex stdout — sandbox blocked file write), `.agents/review-2026-04-27/logs/proposal-review/gemini-adversarial.md` (returned via stdout, bridge unavailable).
