## Why

We bundled a Claude Code skill (`vscode-config`) into this repo (currently shipped as `vscode-config.skill` and `docs/vscode-config.skill`) to give Claude authoritative VS Code configuration knowledge plus three audit/validation/diff bash tools. A multi-dimensional adversarial review (Security + Architecture + Testing reviewers, plus a separate Gemini large-context pass on 2026-04-30 → 2026-05-01) surfaced 15 distinct findings: 0 Critical, 3 High, 7 Medium, 5 Low. The Highs and one Medium with a security implication are validation/audit-bypass surfaces or correctness foundations that should not ship as-is to anyone consuming the skill bundle.

Specifically:

- **CR-001 (High, Architecture)** — the SKILL.md `description` is ~1,650 chars enumerating filenames, OSes, and example phrasings. It is loaded into context on every turn and risks both overtriggering on adjacent JSON tasks and incurring per-turn token cost. A tighter, intent-focused description fits the progressive-disclosure pattern Anthropic recommends.
- **CR-002 (High, Testing)** — the bats `assert_contains` helper used `[[ "$output" != *"$needle"* ]]`, treating the needle as a glob. `assert_contains "[python]"` was passing for the wrong reason in `vscode-profile-diff.bats:126`. All future bracketed assertions are silently false-pass.
- **CR-003 (High, Testing)** — the three scripts now export `LC_ALL=C` for deterministic `comm`/`sort` set operations. No test pins the behavior under a non-C locale; a regression that drops the export would not be caught.
- **CR-005 (Medium, Security)** — `STRIPPED="$(awk -f jsonc-strip.awk < "$FILE")"` round-trips file content through a shell variable, which silently truncates at the first NUL byte (POSIX command substitution). A crafted `extensions.json` could hide additional `recommendations` after a NUL such that `vscode-extensions-audit` validates clean while VS Code parses the full file. The fix is one shared awk guard since all three scripts share `jsonc-strip.awk`.
- **CR-015 (Low, Architecture)** — SKILL.md hardcodes "covering 89 cases", which we already had to bump from 75 once. Magic numbers in narrative docs drift.

Bundling these into one OpenSpec change keeps the audit trail coherent and forces a contract for the skill's behavior, so future regressions surface at PR review rather than at the next adversarial pass.

## What Changes

- **NEW capability `vscode-config-skill`** — establish an authoritative spec for what the bundled skill must guarantee: progressive disclosure, deterministic shell tooling, and validation/audit safety. Findings without a contract simply re-occur.
- Tighten the `vscode-config` SKILL.md frontmatter `description` to a single paragraph (~400-600 chars) that states purpose and a generic intent trigger, not an enumerated phrase list. Move example phrasings to the `## When this skill applies` section in the body.
- Replace the bats `assert_contains` / `assert_not_contains` helpers in `scripts/tests/test_helper.bash` with literal-match versions backed by `grep -qF` so brackets, globs, and regex metacharacters are treated as literals.
- Add a per-script bats test that runs each tool under a non-C locale (e.g. `LC_ALL=tr_TR.UTF-8`) and asserts identical output, so removing the `LC_ALL=C` export trips a test.
- Add a NUL-byte guard at the top of `jsonc-strip.awk` that emits an error and exits non-zero on raw `\0` bytes. Centralizes the fix for all three downstream tools.
- Remove the hardcoded test count from SKILL.md (`scripts/tests/` description); replace with a stable phrasing that doesn't drift.

## Out of Scope

The seven Medium and four other Low findings (cross-file consistency on profile path resolution, exit-code unification across the three scripts, MCP `inputs[]` schema validation, missing dependency / unreadable-file / broken-symlink test coverage, dead awk pass in `vscode-jsonc-validate`, JSONC stripper Unicode-key tests, extensions-audit allowlist edge-case tests, and the unquoted `$TMPFILES` cleanup hazard) are tracked but deferred. They will be batched into a follow-up change `harden-vscode-config-skill-followups` once this lands. Skipping them now keeps this change focused on the items that affect ship-readiness.

## Impact

- **Skill consumers (Claude itself):** The trimmed description reduces always-loaded context cost on every turn and reduces overtrigger on adjacent JSON tasks. Same body, same references, same scripts behaviorally — except the NUL-byte guard, which rejects malformed input that would previously have been silently truncated.
- **Skill maintainers:** Adding a test, locale regression coverage, and the literal-match assertion helper means future regressions to recently-applied hardening are caught by `bash scripts/run_tests.sh`. The new `vscode-config-skill` capability documents the contract so reviewers have something to point at.
- **Repo CI:** No new CI surface required; `bash scripts/run_tests.sh` continues to be the gate.
- **Downstream callers of the bundled scripts:** If anyone in the wild was passing files with raw NUL bytes (rare; not valid JSONC anyway), they now see an explicit error rather than a silent truncation. This is desired behavior.

## Validation

`openspec validate harden-vscode-config-skill --strict` after authoring deltas. `bash scripts/run_tests.sh` in the skill's `scripts/` dir to verify all bats tests still pass. Re-spawn the Gemini and Codex large-context reviewers to confirm CR-001 through CR-005 are resolved before archiving.
