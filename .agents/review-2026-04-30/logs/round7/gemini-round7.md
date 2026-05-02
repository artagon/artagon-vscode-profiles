# Round 7 review — written by Claude (gemini-bridge.js still absent from repo)

## Re-verification of Round-6 fixes
- A. compose-profile-settings/spec.md:22 reads `<segment>(/<segment>)?\.jsonc` (single optional sub-dir) — matches scripts/compose-settings.sh:14 `^[a-zA-Z0-9._-]+(/[a-zA-Z0-9._-]+)?\.jsonc$`. PASS.
- B. scripts/git-hooks/pre-commit:37 awk filter is `$1 ~ /[?MDARC]/ || $2 ~ /[?MD]/`. PASS.
- C. scripts/install-hooks.sh:22-32 — exec-bit loop now precedes the "already installed" success print. PASS.
- D. tasks.md §10.2 says "13 test groups"; §8.4 covers all 10 leaves (java-{gradle,maven,profile,spring} + rust-profile, both crisp/retina). PASS.

## Findings (prioritized)

### MEDIUM — CHANGELOG.md is stale and incomplete (CHANGELOG.md:1-17)
The "Unreleased" section only lists Workspace Trust, fontVariations, atomic compose, and `@extends` containment. It is missing every other behaviorally-relevant change: extension-id allowlist (§3), import-profile PROFILE_ID openssl/secrets switch (§4), check-extension-compatibility.sh repair (§5), open-profiles failure surfacing + validate-json TMPDIR default (§6), .cache/ untracking (§7), CI workflow + install-hooks.sh + pre-commit dirty-tree guard (§8). README's "Behavior Change Notice" links here as the "complete list of changes" — today the link is misleading. Fix: extend the Changed/Security/Added sections to enumerate the other eight items the PR body already articulates.

### MEDIUM — README "Behavior Change Notice" omits dev-facing user impact (README.md:19-23)
Notice covers only the Workspace Trust flip. Users who ran the toolchain locally before this branch will see two new failure modes that warrant a one-line each:
1. "If you keep a custom `_overrides/*.jsonc` with `@extends` reaching beyond a single sub-directory, the composer will now reject it."
2. "Local commits now require staged regenerated `_merged/`+`exports/` artifacts (the new pre-commit hook, installable via `bash scripts/install-hooks.sh`)."
Without these, contributors will hit a wall and not know why.

### LOW — PR test plan asserts "13/13 groups" but Test plan checkboxes are unchecked (gh pr view 2)
Cosmetic but worth ticking the boxes (or replacing with "covered by CI run #N") before merge so reviewers see what was actually exercised.

### LOW — Pre-existing artifact-touching commits pulled into this PR (a53bc3f, fba24a2, 41a3a1f "Rust profile optimizations")
The three Rust optimization commits hand-edit `_merged/*` and `exports/*` directly (e.g., a53bc3f modifies only rust-profile artifacts; fba24a2 modifies all 22 _merged + 22 exports). They predate the harden-profile change but are now bundled into PR #2 and not called out in the PR body. Risk: any future dev reading `git log --oneline -- _merged/` will see hand-edits and assume the regen-only invariant has exceptions. Two options: (a) note in the PR description that these three commits hand-edit artifacts and were not regenerated from `_overrides/`, or (b) confirm the `_overrides/rust*` sources were updated in those same commits (they were: fba24a2 touches `_overrides/java-*-base.jsonc` etc., per the file list) — in which case the commit message "Rust profile optimizations" is misleading because it actually rewrites every profile family. Recommend amending the PR body's "Audit trail" to acknowledge the three pre-harden commits.

### LOW — Round-7 idempotence check passed
`bash scripts/compose-settings.sh && git diff --quiet _merged/` -> CLEAN. `bash scripts/export-profiles.sh && git diff --quiet exports/` -> CLEAN. Tasks 9.x and 10.3 hold.

### LOW — Trust-test false-fail risk is real but acceptable (scripts/tests/run.sh:257-268)
The loop iterates every `_merged/*.json` and fails if `untrustedFiles != "prompt"` (MISSING also fails). There is no opt-out today — if a future profile is intentionally added without a Workspace Trust value, this test will block it. Acceptable as written for the current matrix (the spec contract in `secure-shared-defaults/spec.md` makes "prompt" universal), but flag for whoever later introduces a profile that opts out: they will need to extend this test with an allowlist before adding their merged JSON.

### LOW — Spec deltas mention mechanism only as a behavior reference (review item 1)
Search across `openspec/changes/.../specs/*/spec.md` for "via |using |openssl|awk|realpath" surfaces 4 hits, all defensible:
- compose spec line 4 "via the profile symlink" — naming the read path, not implementation.
- compose spec line 37 "via symlink or direct filename" — behavior.
- import spec line 20 "using `code --profile <name> --install-extension`" — this is a contract (the CLI command shape callers can rely on), not internal mechanism. Keep.
- secure-shared-defaults line 19 "using its variable axes" — typography, not impl.
No leaks.

### LOW — Defensive depth: CI step reordering would not silently pass
`.github/workflows/ci.yml` runs compose+export, then `git diff --exit-code _merged/ exports/`. If a future PR removed the diff step but kept compose+export, drift would not be caught. Today the workflow is read-only token (`permissions: contents: read`), and any change to `.github/workflows/` requires PR review — this is appropriate defense in depth. No action.

### INFO — gemini-bridge.js still missing (scripts/gemini-bridge.js)
This review was written by Claude. If the orchestration pattern matters for future rounds, add the bridge script per AGENTS.md.

## Bottom line
Round-6 fixes verified. Two MEDIUM doc gaps remain (CHANGELOG and README behavior notice). One LOW PR-narrative gap (pre-existing Rust commits not called out). Code is otherwise clean.
