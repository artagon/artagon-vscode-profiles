# Round 6 final adversarial review (gemini-bridge unavailable; performed via direct file inspection)

Branch: feature/github-workflows-profiles-and-tooling-hardening
Commits 9b511ca..8f33bbb

## Re-verifications (round 4/5 fixes)

A) HIGH compose top-level FAILED tracking: VERIFIED.
   scripts/compose-settings.sh:141-162 — FAILED=() declared, every merge_one
   captured (`if ! merge_one "$n"; then FAILED+=("$n"); fi`), prints diagnostic
   and `exit 1` when non-empty. Loop continues on failure (no early `set -e`
   abort in caller because the call is on the LHS of `if !`). Confirmed.

B) MEDIUM symlink test for all 10 leaves: VERIFIED.
   scripts/tests/run.sh:230-243 — iterates {java-gradle, java-maven,
   java-profile, java-spring, rust-profile} × {crisp, retina}; asserts -L and
   readlink == "<stack>-base.jsonc". 10 leaves covered.

C) MEDIUM @extends regex: VERIFIED.
   scripts/compose-settings.sh:14 EXTENDS_NAME_RE='^[a-zA-Z0-9._-]+(/[a-zA-Z0-9._-]+)?\.jsonc$'
   Single optional slash; matches design intent. Note: spec delta
   compose-profile-settings/spec.md:18 still documents it as `(/<segment>)*`
   which permits arbitrary depth. Spec-vs-impl drift; impl is stricter. Low
   priority but should be reconciled before archive (option: relax impl back to
   `*` since spec is the contract, OR tighten spec to match impl `?`).

D) MEDIUM pre-commit dirty-tree guard: VERIFIED at scripts/git-hooks/pre-commit:37
   `git status --porcelain _merged/ exports/ | awk '$1 ~ /[?MD]/'`
   Caveat below in finding 2.

E) LOW CHANGELOG link: VERIFIED. README.md:23 "See [CHANGELOG.md](./CHANGELOG.md)".

F) LOW prompt-lands-in-merged test: VERIFIED.
   scripts/tests/run.sh:245-255 iterates all _merged/*.json and asserts
   `."security.workspace.trust.untrustedFiles" == "prompt"`.

G) install-extensions/import-profile jq materialization: VERIFIED.
   scripts/install-extensions.sh:112-115 (EXT_LIST="$(jq …)" || …) and
   scripts/import-profile.sh:112-115 (same pattern). Process substitution
   replaced as advertised.

H) check-extension-compatibility awk id matching: VERIFIED.
   scripts/check-extension-compatibility.sh:200-204 splits on '@', literal
   string compare on tolower($1). The previous regex bug (where '.' in
   'ms-vscode.cmake-tools' acted as any-char) is gone.

I) compose CYCLE_STACK depth-balanced: VERIFIED.
   scripts/compose-settings.sh:34-52 — global array; collect_overrides pushes,
   calls _collect_inner, captures rc, unsets last entry on a single return
   path. The `+("${CYCLE_STACK[@]+"${CYCLE_STACK[@]}"}")` expansion handles
   empty-array under set -u correctly.

J) E2E injection tests invoke production scripts: VERIFIED.
   scripts/tests/run.sh:158-207 — Path A install-extensions.sh against fixture
   profile, Path B import-profile.sh with poisoned bundle, Path C vspcli
   --install-ext. All assert exit non-zero AND grep "rejected extension id".

## NEW findings (round 6)

1. SPEC-VS-IMPL @extends regex drift (LOW, file: openspec/changes/.../specs/compose-profile-settings/spec.md:18 vs scripts/compose-settings.sh:14).
   Spec contract: `<segment>(/<segment>)*\.jsonc` — arbitrary depth.
   Implementation: `^[a-zA-Z0-9._-]+(/[a-zA-Z0-9._-]+)?\.jsonc$` — at most one
   slash. Test fixtures only exercise rejection cases (traversal, abs, ~), not
   acceptance of multi-segment paths. Either:
   - tighten spec scenario wording to "single optional sub-directory" (matches
     today's actual repo usage which is zero-deep), or
   - relax regex back to `(/[a-zA-Z0-9._-]+)*` to honor spec.
   Recommend tightening spec since the use case isn't there.

2. PRE-COMMIT dirty-tree awk regex GAP (LOW-to-MEDIUM, scripts/git-hooks/pre-commit:37).
   `awk '$1 ~ /[?MD]/'` matches when column 1 contains ?, M, or D.
   Verified empirically:
     - "?? foo"  matched
     - " M foo"  matched (space + M; M is in column 2 but $1 in awk default
       FS=whitespace makes $1="M")
     - "AM foo"  matched
     - "A  foo"  NOT matched
   Consequence: a NEW file created by compose/export (e.g., a brand-new
   _merged/<foo>.json that didn't exist before) appears as either "?? foo"
   (untracked) or "A  foo" (staged add). The former IS caught (?), the latter
   is NOT. In practice the hook runs compose which produces the file; if the
   user had not pre-staged anything new, the file shows up as "?? foo" and is
   caught. If they ran `git add -A` before invoking the hook (rare), they'd
   stage it, the hook would re-run compose (idempotent), and the staged "A  "
   line would be invisible to the awk filter. Severity: low — the staged-add
   case means the user already has the file staged, so the symptom (missing
   regen) doesn't apply. Still, the regex would be more honest as
   `awk '$1 ~ /[?MDA]/ || $2 ~ /[MD]/'` to make intent explicit.

3. CI cross-OS determinism (LOW-to-MEDIUM, .github/workflows/ci.yml:25-32 and scripts/compose-settings.sh:17-24, 119, 127).
   `realpath` shim falls back to python; Linux runner has GNU realpath, macOS
   developer has BSD. Both should produce the same canonical path FOR PATHS
   INSIDE THE REPO since neither inserts symlink-only normalization differences
   for repo-local paths. mktemp templates: `mktemp` in compose-settings.sh:119
   uses no template (default). `mktemp -d -t vscode-tests-XXXXXX` in tests
   works on both. The `_merged/<name>.json.tmp.$$` sibling temp at line 127 is
   deterministic in name but the file is renamed away before commit, so PID
   non-determinism does not leak into tracked output. CONCLUSION: no spurious
   diff failures expected. JSON key order: jq on both OSes is the same binary
   semantics; output ordering identical.

4. install-hooks.sh executable bit not checked (LOW, scripts/install-hooks.sh:22-25).
   The "already installed" branch returns 0 without verifying that
   $ROOT/scripts/git-hooks/pre-commit is `-x`. If a contributor's umask, a
   tarball checkout, or `git update-index --chmod=-x` cleared the bit, hooks
   silently won't run. Suggested addition (3-line hardening, not blocking):
     hookfile="$ROOT/$TARGET/pre-commit"
     if [ -e "$hookfile" ] && [ ! -x "$hookfile" ]; then
       echo "install-hooks: $hookfile is not executable; chmod +x $hookfile" >&2
       exit 1
     fi
   Note: git itself does NOT require hooks to be executable on Windows, but
   does on POSIX. Defer to follow-up; non-blocking.

5. CHANGELOG/README narrative consistency: CONSISTENT.
   README.md:21 ("Starting with the harden-profile-tooling-and-pipeline change")
   and CHANGELOG.md:5-9 ("BREAKING ... untrustedFiles to prompt") agree on
   wording, scope ("first time you open an untrusted folder"), and trigger
   ("Workspace Trust prompt before running language servers, tasks, debug
   launches, or formatters"). No contradictions.

6. CHANGELOG completeness vs what shipped (LOW).
   CHANGELOG.md:5-18 lists: Workspace Trust default, SECURITY.md update,
   fontVariations restoration, atomic compose, @extends rejection rules.
   Missing entries that were also part of this proposal:
     - extension-id allowlist (sections 3.1-3.6 of tasks.md)
     - import-profile PROFILE_ID hardening (4.1)
     - check-extension-compatibility rewrite (5.x)
     - open-profiles failure surfacing (6.1)
     - validate-json TMPDIR default (6.2)
     - .cache untracking (7.x)
     - CI workflow (8.1)
     - install-hooks.sh (8.2)
   The CHANGELOG itself acknowledges "Further entries will land as the change
   progresses; this section will be finalized at archive time." (line 18).
   Acceptable for an unreleased section, but call this out in the PR
   description so reviewers know the CHANGELOG is intentionally incomplete and
   will be backfilled at archive.

7. Pre-existing-on-branch context (NEW).
   The 4 commits before 9b511ca (174fef4 GitHub Workflows feature; a53bc3f /
   fba24a2 / 41a3a1f Rust optimizations) are part of this branch and will
   ship in the PR. They are NOT covered by the harden-profile-tooling change
   proposal. The PR description should explicitly call out:
     - feat: github-workflows-(crisp|retina) profile family added
     - perf: rust profile optimization tweaks
   so reviewers don't conflate them with the hardening work and ask "why is
   this in a hardening PR?". Suggest section "Bundled changes outside the
   hardening proposal" in PR body.

8. Proposal/tasks vs shipped reality (LOW-MEDIUM).
   - tasks.md:54 "Add a tests/run.sh assertion that
     _overrides/java-spring-{crisp,retina}.jsonc are symlinks" is satisfied
     and EXPANDED in code (10 leaves, not 2). Tasks file should be updated to
     reflect the wider coverage before archive — proposal accuracy matters for
     audit trail.
   - tasks.md:55 "constraint to openspec/project.md documenting the symlink
     trick" — I did not verify this was committed; check
     openspec/project.md exists with the constraint before archiving.
   - tasks.md:67 says "verified by md5-hashing all artifacts before and after
     a second regen" — no test asserts this; acceptable since it's a one-time
     manual check, but call out as such or move to design.md if it's
     intent-not-mechanism.
   - 13 spec deltas claim in your prompt vs 8 actual under
     openspec/changes/.../specs/. Either the prompt was wrong or 5 deltas were
     planned but not authored. Confirm: 8 directories present —
     audit-profile-extensions, compose-profile-settings, import-profile-bundles,
     install-profile-extensions, manage-profile-cli, open-profiles,
     secure-shared-defaults, validate-profile-json. Matches proposal.md:38-46
     exactly (8 listed). Prompt's "13" appears incorrect; this is fine.

9. PR description suggestion update (vs your earlier draft):
   Should now include:
     - 6 rounds of adversarial review baked in (logs in
       .agents/review-2026-04-{27,28}/)
     - END-TO-END injection tests against three production code paths
     - depth-balanced cycle stack (vs string-packed in earlier rounds)
     - awk-based id matching in compat checker (was regex)
     - jq materialization in install/import (was process substitution)
     - dirty-worktree pre-commit guard
     - Bundled non-hardening changes (github-workflows profiles + rust perf)
   Test plan additions:
     - run scripts/tests/run.sh (12 groups now; was 11 in tasks 10.2)
     - run scripts/install-hooks.sh on a fresh clone, then `git commit
       --allow-empty -m "test"` to confirm hook fires
     - import a poisoned .code-profile bundle and confirm exit non-zero

## NO REMAINING BLOCKERS

The 4 LOW findings above (regex spec drift, awk regex tightening, install-hooks
exec-bit check, CHANGELOG completeness statement in PR body) are all
post-merge follow-up material. Recommend opening PR.
