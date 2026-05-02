## Context

Three independent reviewers (Claude in-session, Codex via plugin, Gemini via plugin — though the bridge fell back to a host-Claude pass) audited the repo on 2026-04-27 against the working-tree diff on `feature/github-workflows-profiles-and-tooling-hardening`. A second adversarial pass on the original draft of this change ran 2026-04-28 and surfaced wrong-shape fixes, missing spec contracts, implementation-leaking scenarios, silently-dropped findings, and task-ordering gaps. This document captures the technical decisions after that second pass. Logs at `.agents/review-2026-04-27/logs/code-review/` and `.agents/review-2026-04-27/logs/proposal-review/`.

The underlying code is small: 1.1k LOC of bash plus ~30 JSONC fragments. A single proposal is more honest than splitting one finding per change because most fixes are one-line and several depend on the CI workflow this change introduces.

## Goals / Non-Goals

Goals:
- Restore VS Code Workspace Trust as the default for all shipped profiles, contracted as a spec requirement.
- Restore the variable-font policy regression caught in the original review.
- Make the compose → merged → symlinked pipeline crash-safe.
- Make the `_overrides/@extends` chain robust against malformed values, cycles, and path traversal.
- Stop trusting arbitrary extension IDs from JSON files, with a single sourced helper that all install paths share.
- Stop suppressing real failures across the script suite.
- Make the unspecified compatibility checker a real, contracted capability that works.
- Stop tracking per-machine state in git.
- Add a CI gate so the next regression of any of the above is caught at PR time, not by users.

Non-goals (deferred):
- Collapsing the crisp/retina axis (architectural change with broad downstream effects).
- Removing `_merged/` and `exports/` from version control (workflow change; depends on CI being in place).
- Renaming `_overrides/*.jsonc` to `*.json` to match their actual strict-JSON content.
- Re-evaluating word-splitting hardening inside `check-extension-compatibility.sh` JSON assembly until §6 lands.
- Pinning extension versions in `extensions.json` (separate proposal).

## Decisions

### Decision: Retire the Spring inheritance finding (false positive)
Implementation revealed `_overrides/java-spring-{crisp,retina}.jsonc` are git symlinks (mode `120000`) pointing at `java-spring-base.jsonc`. Reads through the symlinks return the base content, so all three reviewers (Claude, Codex, Gemini) reported "duplication" or "wrong base" because they were inspecting linked content while the files themselves are pointers. The composer's base-selection at `compose-settings.sh:38-39` correctly uses the *filename* (`*retina*` vs default crisp), not the content, so the symlink trick already achieves zero duplication. Replacing it with explicit `@extends` shims would add an indirection layer for no behavioral gain. Finding retired.

Lesson for the cycle-detector requirement in `compose-profile-settings`: the spec should still gain the cycle-detection scenario, but the original Spring "fix" that motivated it does not need to ship.

### Decision: Workspace Trust + variable-font policy as a new `secure-shared-defaults` capability
Both belong together — they are properties of the shared editor base, not of any one profile. Modeling them as a capability ensures any future change to the shared base must add or update a contracted requirement, surfacing the regression risk in PR review rather than silently in user installs.

### Decision: Atomic merge writes via temp + rename in the same directory
Rationale: `profiles/<name>/settings.json` is a symlink to `_merged/<name>.json`, which is the file VS Code reads at startup. Truncate-then-write means any interrupt or jq parse error leaves a live profile reading an empty file. `mv` on the same filesystem is atomic on POSIX. The spec states the *behavior* (no partial reads); the temp+rename is the implementation captured here.

Alternatives considered:
- `flock` around the write — heavier, only protects against concurrency, not interrupts.
- Skip the symlink and copy at compose time — defeats the point of making `_merged/` the single source of truth.

### Decision: Path containment for `@extends`
Reject `..`, leading `/`, and backslashes. This is conservative; legitimate references stay inside `_overrides/` and use bare filenames. This blocks both accidental escape (relative path through symlinked dirs) and intentional escape (a poisoned override file pointing at `/etc/passwd`).

### Decision: Single sourced helper at `scripts/lib/extension-id.sh`
Three scripts must enforce the same regex (`install-extensions.sh`, `import-profile.sh`, `vspcli`). A single sourced file plus a CI drift check (§3.6, §10.4) prevents the regex from drifting between consumers and the spec.

Pattern: `^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$`. Permits any published marketplace ID (verified against all 64 unique IDs across every profile in the second review pass), denies shell metachars, denies path traversal.

### Decision: Audit exit code contract
- `0` clean run, no findings
- `1` (or any other non-2 non-zero) findings present
- `2` reserved for CLI misuse only

The helper function `check_compatibility` returns 1 (incompatible) and 2 (unknown) internally, but the script process exit code translates these so 2 only ever means "you invoked me wrong." This avoids the trap where automation cannot distinguish "you passed a bad flag" from "an extension is in unknown state."

### Decision: Workspace Trust changes are BREAKING — documented in README
Flipping `untrustedFiles` from `"open"` to `"prompt"` is a behavior change visible to every user on first folder open per profile. Task §1.4 adds a Migration / Behavior Change section to README. The Workspace Trust prompt is itself sufficient migration documentation; the README addition tells existing users why they're suddenly seeing it.

### Decision: Task ordering — security defaults first, regenerate once at the end
With the Spring section retired, §1 is now the shared-base edits (Workspace Trust + variable fonts + SECURITY.md + README), followed by code edits §2–§6, hygiene §7, CI §8, and a single regenerate-and-export pass at §9. Validation §10 closes the loop.

### Decision: CI runs on Linux, asserts on both `_merged/` AND `exports/`
The first draft only diffed `_merged/`. Both are committed and both regenerable, so both are gated. macOS-specific `mktemp` patterns in `compose-settings.sh` are explicitly checked at §2.1. Runner pinned to `ubuntu-latest`. Step order in CI: regen → diff → validate → tests, so tests always see freshly composed artifacts and cannot pass against stale `_merged/`.

### Decision: Symlink invariant for Spring leaves is project-level
With §1 retired, `_overrides/java-spring-{crisp,retina}.jsonc` remain git symlinks (mode `120000`) pointing at `java-spring-base.jsonc`. This is load-bearing: the composer's filename-keyed base selection means each leaf inherits the right shared-DPI base while sharing every other key. A future contributor running `cat > _overrides/java-spring-crisp.jsonc` would silently break propagation. The invariant is captured in `openspec/project.md` constraints and asserted in `scripts/tests/run.sh` via `[[ -L … ]]`.

### Decision: Hooks installation is shipped as an idempotent installer script
Earlier draft only added a documentation snippet to CONTRIBUTING.md. That is not idempotent and silently clobbers users who already set `core.hooksPath` for husky/pre-commit/etc. Ship `scripts/install-hooks.sh` that checks the existing value, refuses to overwrite a foreign hook path with a clear message, and exits zero when the path is already correct.

### Decision: Defer the crisp/retina collapse and the `.jsonc`/.json rename
Both are touched-everything-style refactors that would balloon this change's review surface and conflict with the urgent fixes. Deferred to follow-up proposals once CI is in place.

## Risks / Trade-offs

- **Risk:** The Workspace Trust flip surprises users on next open.
  - **Mitigation:** README migration entry (§1.4); the prompt itself is the runtime documentation.
- **Risk:** The path-containment regex is too strict and rejects a legitimate `@extends` value.
  - **Mitigation:** All current overrides use bare filenames inside `_overrides/`; verified before proposal write. New uses must stay inside `_overrides/`, which matches the existing convention.
- **Risk:** The CI step `git diff --exit-code _merged/ exports/` makes every shared-base PR require a paired regen commit.
  - **Mitigation:** Document this in CONTRIBUTING.md as part of §8.2; add a single-command regen helper to `scripts/vspcli` if needed in a follow-up.
- **Risk:** Spring symlink invariant is implicit and easy to break with non-symlink-aware tools.
  - **Mitigation:** `tests/run.sh` asserts `_overrides/java-spring-{crisp,retina}.jsonc` are symlinks, and `openspec/project.md` lists the invariant under Constraints.
- **Risk:** `git rm -r --cached .cache/` confuses users with stale local markers.
  - **Mitigation:** Cache markers are not load-bearing; they regenerate on next `open-profiles.sh` run. PR description calls this out.

## Migration Plan

1. Apply §1 (security defaults). User-visible behavior change.
2. Apply §2–§6 (pipeline correctness, helpers, audit fixes, surfaced failures).
3. Apply §7 (gitignore + remove cached).
4. Apply §8 (CI workflow + hook docs).
5. Run §9 (single final regenerate of `_merged/` and `exports/`) so the diff is coherent.
6. Run §10 (validation).
7. Open PR. Description must include: BREAKING (Workspace Trust), README migration entry pointer, link to the review logs in `.agents/`.
8. Rollback path: revert the merge commit. Cache markers are per-machine and need no restore.

## Open Questions

- Should `import-profile.sh` reject *unsigned* `.code-profile` bundles, or only validate IDs? Probably validate-only for now — bundle signing is a separate design topic.
- Should the CI workflow also run `scripts/check-extension-compatibility.sh` against all profiles? It depends on a working `code` CLI, which the standard runner does not have. Consider a separate optional job once §6 lands.
- Is there a project-policy decision on whether `_merged/` and `exports/` should be checked in long-term? Not blocking this change, but worth answering in the follow-up.
