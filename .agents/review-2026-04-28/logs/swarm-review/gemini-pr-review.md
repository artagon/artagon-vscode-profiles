# PR review — feature/github-workflows-profiles-and-tooling-hardening (round 4)

Branch: `feature/github-workflows-profiles-and-tooling-hardening`
Change: OpenSpec `harden-profile-tooling-and-pipeline` (9 commits)
Date: 2026-04-28
Bridge: `scripts/gemini-bridge.js` is **not present** in this repo. Reviewed directly with file inspection; flagged below.

## Top findings (prioritized)

### 1. BLOCKER — CI hardening (SHA pin + `permissions:`) is uncommitted
`/Users/.../.github/workflows/ci.yml` shows the SHA-pinned `actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd` and `permissions: contents: read` only as **unstaged working-tree changes** (`git diff .github/workflows/ci.yml`). The committed file at `70b3efa` still has `uses: actions/checkout@v4` and **no `permissions:` block**. Net effect on the PR as pushed: workflow runs with the default token scope (writeable) and a moveable tag. This contradicts the proposal's "supply chain hardened" claim. **Commit the diff before merge.**

### 2. HIGH — `pull_request` trigger executes shell scripts from forks under default `GITHUB_TOKEN`
`ci.yml:6` registers `pull_request:` (not `pull_request_target`, which is correct), but the job runs `scripts/compose-settings.sh`, `scripts/export-profiles.sh`, `scripts/validate-json.sh`, and `scripts/tests/run.sh` — all from the PR ref. A malicious PR can replace any of these scripts. The default token has limited scope on `pull_request` from a fork (read-only by default since 2023), but there are still no `secrets`-exfil vectors only because none are declared. Once `permissions: contents: read` is committed (finding #1), this becomes acceptable. **Make the permissions block landing a hard prerequisite to enabling the workflow on PRs from forks.**

### 3. HIGH — `git diff --exit-code _merged/ exports/` will fail under cycle/error paths in mode-of-use
`scripts/compose-settings.sh:97-104` (mv-then-rm) leaves the previous `_merged/<name>.json` in place when `jq` fails — good. But `merge_one` returns 1 on failure while the top-level loop calls it via `for d in "$PROFILES_DIR"/*; do ... merge_one "$n"; done` with `set -e`, so the **first** failing profile aborts the loop and remaining profiles are never re-emitted. Combined with the CI step ordering (regen → diff), a single bad override silently masks all downstream merges. Consider switching the loop to track failures and exit non-zero **after** processing all profiles (same pattern as `open-profiles.sh` 6.1).

### 4. MEDIUM — Symlink invariant test is narrower than the actual invariant
`scripts/tests/run.sh:222-227` only asserts `java-spring-{crisp,retina}.jsonc → java-spring-base.jsonc`. `find _overrides -type l` returns **10 symlinks**, all of the same `<name>-{crisp,retina}.jsonc → <name>-base.jsonc` shape (`java-gradle`, `java-maven`, `java-profile`, `java-spring`, `rust-profile`). The proposal scoped the invariant narrowly, but the same regression risk applies to all five families. Recommend extending the loop to all 10 leaves, since the cost is one `for` loop and the safety scales linearly.

### 5. MEDIUM — Path-containment regex permits subdirectory depth not used in the codebase
`scripts/compose-settings.sh:13` permits `<segment>(/<segment>)*\.jsonc`. Inventory of every `@extends` value in `_overrides/` shows the deepest legitimate use is `ai/copilot.jsonc` — exactly **one** subdirectory level. The current regex permits `a/b/c/d/...jsonc`, which expands attack surface for symlink/realpath edge cases under the existing containment guard. Tighten to `^[a-zA-Z0-9._-]+(/[a-zA-Z0-9._-]+)?\.jsonc$` (max one slash). Defense in depth; not exploitable today thanks to `OVR_REAL` containment.

### 6. MEDIUM — TOCTOU/auto-stage gap in pre-commit hook
`scripts/git-hooks/pre-commit` runs `compose-settings.sh` and `export-profiles.sh` before the commit. If a contributor edits a fragment and runs `git commit -a`, compose updates `_merged/*.json` **after** staging — those updates land as **unstaged changes after the commit succeeds**. The hook does not `git add` the regenerated artifacts. Result: every commit that touches a fragment will leave the worktree dirty and CI's `git diff --exit-code` will fail on the *next* commit unless the contributor manually re-stages. Either auto-stage `_merged/`+`exports/` after the regen, or fail the hook with a clear "regenerate and re-stage" message.

### 7. LOW — README does not link CHANGELOG.md
`README.md` has no link to `CHANGELOG.md`. The "About" section at line 5 and the badge row do not mention release notes. Discoverability for the BREAKING Workspace Trust flip depends on contributors finding `CHANGELOG.md` by accident. Add a "Changelog" link in the About section or near the badges. CHANGELOG format itself is fine — close enough to keepachangelog.com (Unreleased + Changed/Security sections).

### 8. LOW — OpenSpec archive will create two new capability dirs; no orphans
`openspec/specs/secure-shared-defaults` and `openspec/specs/audit-profile-extensions` do not exist yet (NEW capabilities per `proposal.md`). The other 6 deltas (`compose-profile-settings`, `import-profile-bundles`, `install-profile-extensions`, `manage-profile-cli`, `open-profiles`, `validate-profile-json`) all have existing dirs in `openspec/specs/`. `openspec archive harden-profile-tooling-and-pipeline` should succeed cleanly. **No blocker.**

### 9. LOW — Spec mechanism leaks (minor)
`secure-shared-defaults/spec.md` line discussing `editor.fontVariations` says *"the boolean form `true` is the documented automatic translation mode and is the form chosen here"* — borderline implementation detail (justifies a *value*, not a mechanism). `compose-profile-settings/spec.md` line discussing cycle detection mentions "resolved real path (after symlink resolution)" — also borderline; arguably a contract about identity, not a mechanism. Not worth blocking; review at archive time.

### 10. LOW — Self-identified gaps are appropriate as follow-ups
- No SIGINT mid-compose test: the atomic-write contract in `compose-profile-settings/spec.md` covers the *behavior* (no partial file exposed). Testing requires a fork+kill harness; reasonable to defer.
- No `_merged/*.json == "prompt"` post-flip assertion: trivially derived from the existing `compose-settings.sh` test plus the merged files being committed — recommend adding a one-liner `grep -L '"prompt"' _merged/*.json | xargs -I{} false` in `tests/run.sh` to make the contract visible. Not a blocker.

### 11. INFORMATIONAL — Badge URL canonical case
Both `https://github.com/Artagon/...` and `https://github.com/artagon/...` return HTTP 200 (verified via curl). GitHub redirects the badge service transparently. **No action needed**, but if you ever rename the org, both badges may break in lockstep — consider switching to the lowercase canonical form preemptively (`README.md:7`).

## Summary

Two items must land before merge: **(1)** commit the SHA-pin + `permissions:` block in `ci.yml`, **(2)** fix the silent skip in `compose-settings.sh` top-level loop. Everything else is hardening or polish that can ship as follow-ups.

The bridge script `scripts/gemini-bridge.js` referenced by the agent contract is missing from the repo — recommend adding it (or removing the contract from the agent prompt) so future reviews stay reproducible.
