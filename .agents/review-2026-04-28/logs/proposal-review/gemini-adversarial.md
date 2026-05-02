# Adversarial review — revision 3 — host-Claude pass (Gemini bridge unavailable)

Top findings, prioritized.

## B1 (BLOCKER) — The Spring symlink retirement is wrong; @extends cycle WILL fire on first compose

`git ls-files -s _overrides/java-spring-{crisp,retina,base}.jsonc` shows crisp and retina ARE symlinks to `java-spring-base.jsonc` (mode 120000, identical hash `c7dbf2…`). Reading any of the three files returns the SAME content, including `"@extends": ["java-profile-base.jsonc"]` (`_overrides/java-spring-base.jsonc:3`). That is fine for `java-spring-crisp` / `java-spring-retina` (they extend `java-profile-base.jsonc`, not themselves), so there is NO cycle today.

But the proposal in `compose-profile-settings/spec.md:45-47` adds a "Cycle in @extends chain → exits non-zero" requirement, and `tasks.md:13` adds a regression test. With the symlink trick in place, on the *first run that resolves @extends from `java-spring-base.jsonc` itself* (e.g. anyone composes the base directly, or a future leaf points at `java-spring-base.jsonc`), the resolver will read through the symlink, see the leaf's @extends, and behave correctly — UNLESS the resolver normalizes by `realpath` and dedupes, in which case `java-spring-crisp.jsonc` and `java-spring-base.jsonc` are the same inode and a self-reference becomes possible. The proposal does not specify *how* the cycle detector identifies a node (filename string vs. resolved inode). Without that, the cycle scenario is untestable and the symlink invariant is fragile. Add a requirement: "Cycle detection SHALL use resolved real paths so that symlink aliases are treated as the same node."

## B2 (BLOCKER) — Variable-font requirement contradicts shipped state, no Modified-Files task

`secure-shared-defaults/spec.md:14-19` requires `editor.fontVariations` to be enabled. Both `_shared/editor-crisp.jsonc:35` and `_shared/editor-retina.jsonc:35` currently set `"editor.fontVariations": false`. `tasks.md:4` says "Restore `editor.fontVariations` to `true`" but the only artifact regen is at §9. Reviewer note: spec contract claims "SHALL enable" — the truthy value is `true`, but VS Code's setting actually takes an object/array of variations; `true` is accepted as "use defaults" only on some versions. Verify the intended literal value and pin it in the task ("set to `true`" vs "set to a non-empty object"). Today's `false` directly violates the proposed spec, so until §1.2 + §9 land, the change is internally inconsistent.

## H1 (HIGH) — design.md still references retired/never-existing section numbers (§4.6, §11.4, §2.4)

`design.md:48` cites "§4.6, §11.4" for the regex drift check. Tasks have no §4.6 (§4 ends at 4.2) and no §11 at all (tasks end at §10). The actual drift check is §3.6 + §10.4. `design.md:60` and `:74` cite "§2.4" for the README migration entry; the README migration task is actually §1.4 (`tasks.md:6`). These are stale references from before the renumbering, and they will mislead an implementer or archiver. Fix: §4.6 → §3.6, §11.4 → §10.4, §2.4 → §1.4.

## H2 (HIGH) — Symlink invariant is now load-bearing but UNDOCUMENTED

With the §1 Spring fix retired, the only thing keeping the spring family duplication-free is the fact that `_overrides/java-spring-{crisp,retina}.jsonc` are git symlinks. A contributor running `cat > _overrides/java-spring-crisp.jsonc` (or any editor that doesn't preserve symlinks — VS Code's "save as" does not on some platforms) silently materializes the file and the next compose will work BUT subsequent edits to `java-spring-base.jsonc` will not propagate. There is no spec contract, no `project.md` entry, no CONTRIBUTING.md note, no test asserting `[[ -L _overrides/java-spring-crisp.jsonc ]]`. Recommendation: add a CI assertion (file mode is symlink) and a one-line note in `openspec/project.md:38-40` "Constraints" listing.

## H3 (HIGH) — Variable-font requirement conflicts with user setting precedence — undefined

`secure-shared-defaults/spec.md:14` mandates `editor.fontVariations` be enabled in shared base. A user who explicitly sets `editor.fontVariations: false` in their User-level settings.json will see Profile-level override win (VS Code Profile settings beat User settings for keys present in the profile). The spec is silent on this. Reviewer's prior question stands: is this a profile mandate, or a default? If default, the spec wording should say "default to" not "SHALL enable". As written, it forbids contributor tooling from ever shipping `false`, even via a future explicit-axes object.

## H4 (HIGH) — `audit-profile-extensions` "Concrete installed-version reporting" is internally contradictory

`audit-profile-extensions/spec.md:21-26` says "SHALL report a concrete version string for every extension that is installed in the audited profile" but `:17-19` says "extension absent in target profile → not installed". Two scenarios mention an extension being absent (covered) and an extension being present (covered). What is missing: an extension that IS installed but for which `code --list-extensions --show-versions` returns an empty/garbage version (happens with broken installs, and with VS Code Insiders quirks). The spec says "SHALL only report `unknown` when VS Code returns no version information" — fine — but the concrete-version requirement's wording "SHALL report" is unconditional. Add: "for every extension … that is installed AND for which VS Code returns a version string." Otherwise the requirement and its sibling are in tension.

## H5 (HIGH) — Hooks task §8.2 documents idempotency but does NOT install — M5 partially deferred without saying so

`tasks.md:51` documents an idempotent snippet in CONTRIBUTING.md. But the *original* M5 was about the install scripts/git-hooks themselves being non-idempotent. Documenting a one-shot snippet for humans to copy-paste is not a fix; it's a workaround. The proposal's Non-Goals list does NOT explicitly defer M5. Either (a) add an actual installer script that is itself idempotent, or (b) add M5 to Non-Goals with rationale. Currently M5 is silently downgraded.

## M1 (MEDIUM) — `audit-profile-extensions` capability never appears in `openspec/specs/`

The proposal "Affected specs" lists `audit-profile-extensions (NEW)` but post-archive there will be no baseline `openspec/specs/audit-profile-extensions/spec.md` to grow into — the change creates the capability under `changes/.../specs/`. Confirm the archive process for an ADDED capability with only `## ADDED Requirements` deltas correctly produces a baseline spec. (Past OpenSpec versions required at least a stub.) The strict-validation pass referenced as "passes" should be re-run after `openspec archive --dry-run` if available.

## M2 (MEDIUM) — Path-containment regex is too narrow

`compose-profile-settings/spec.md:21-22` rejects `..`, leading `/`, and backslashes. It does NOT reject NUL bytes, leading `~`, or URL-encoded traversal (`%2e%2e`). For jq parsing of strict JSON the encoded forms are not interpreted, so this is mostly fine — but `~/.ssh/...` would slip through and resolve at the shell layer. Either (a) anchor with `^[a-zA-Z0-9._-]+\.jsonc$` (tightest), or (b) add `~` to the rejection list. Today's wording leaves an obvious bypass.

## M3 (MEDIUM) — `import-profile-bundles` keeps the OLD "continue even if an extension install fails" clause un-modified

`openspec/specs/import-profile-bundles/spec.md:44-53` (existing baseline) says "continue even if an extension install fails." The delta at `changes/.../specs/import-profile-bundles/spec.md:19-29` MODIFIES "Extension installation" to require non-zero exit after the loop. Good. But the delta paste did not delete or supersede the older "Scenario: Extension install failure → continues" — it replaces it with "records the failure, continues … exits non-zero after." This is correct in the change, but the archiver replacing the requirement means the existing scenario "continue even if an extension install fails" is overwritten. Verify the MODIFIED block contains the COMPLETE requirement (header + scenarios), not a partial. Looking at `:19-29`, the MODIFIED requirement does include both scenarios. OK, but confirm at archive time.

## L1 (LOW) — `tasks.md:50` runs compose+export AFTER `validate-json.sh` and `tests/run.sh`

The CI step order means `tests/run.sh` runs against the committed `_merged/`/`exports/`, then a regen happens. If a test depends on freshly-composed output, you get yesterday's artifacts. Probably safe given current tests, but flag for reviewer: order should be regen → diff → validate → tests, OR tests should not assume freshness.

