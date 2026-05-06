## Context

Generated `.code-profile` bundles include theme/icon-theme extensions in
their `extensions.enabled` list. When a recipient imports a bundle, two
paths exist:

1. `scripts/import-profile.sh` — installs each extension via
   `code --profile <name> --install-extension <id>` *before* VS Code
   reads the profile's `settings.json`. Themes resolve correctly.
2. VS Code's built-in UI import (Settings → Profiles → Import) — only
   *enables* extensions already installed; it does NOT install missing
   ones. The imported `settings.json` references theme names whose
   backing extensions are absent, and the workbench falls through to
   whatever VS Code's resolver picks (typically the built-in default
   theme, but with broken icon glyphs and the wrong syntax palette).

Path 2 is the default user experience — it's the obvious option in the
VS Code UI. Users compare a generated profile to their existing default
profile (which uses the always-present built-in `Dark+`) and conclude
the generated profile is broken. It isn't; the import path silently
degraded it.

The cheap fix is to make `import-profile.sh` robust against partial
extension-install failures by stripping theme keys whose backing
extensions failed. Path 2 (UI import) cannot be patched from this
project — but a documentation nudge plus an export-time notice can
steer users to Path 1.

## Goals / Non-Goals

**Goals:**

- `import-profile.sh` rewrites the imported profile's `settings.json`
  to remove `workbench.colorTheme` and `workbench.iconTheme` whose
  target extensions failed to install, before VS Code first reads
  the profile.
- The rewrite is a no-op when all extensions install successfully.
- `export-profiles.sh` prints a final-line notice naming
  `import-profile.sh` as the canonical import path.
- `README.md` Quick Start ranks `import-profile.sh` above the UI
  import path.
- A regression test asserts the rewrite happens when an extension
  fails to install.

**Non-Goals:**

- Changing `_shared/`, `_overrides/`, or any of the 22 profile
  manifests. The styled-theme look is preserved on the canonical
  install path.
- Patching VS Code's UI import — out of scope, can't.
- Adding a separate "graceful" profile variant. The same bundles
  serve both paths; only the import script changes.
- Theme-extension auto-install attempts (e.g., a retry loop). If
  the user's network or marketplace access is broken, retrying
  doesn't help. The fallback is graceful degradation, not automatic
  recovery.
- A sophisticated extension-to-theme registry maintained out-of-band.
  The mapping table is small (12-ish entries) and lives in the script
  itself; updates ride alongside profile changes.

## Decisions

### Decision 1: Rewrite the imported settings.json, not the bundle

**Choice**: After installing extensions, rewrite the *imported*
`settings.json` under `<VSCODE_USER_DIR>/profiles/<id>/settings.json`
— not the bundle file the user passed in.

**Rationale**: the bundle is an artifact (often shipped via release
asset or repo); mutating it would surprise the recipient. The
imported profile cache is what VS Code reads on first launch; that's
the right surface to fix.

**Alternatives considered**:

- *Mutate the bundle in place*: rejected — surprising and would
  change checksums.
- *Write a sidecar `settings.override.json`*: VS Code doesn't read
  such a file; would require additional plumbing.

### Decision 2: Static label-to-extension-ID map in the script

**Choice**: ship a small built-in table (12-ish entries) in
`import-profile.sh` mapping human-readable theme labels (e.g.,
"Tokyo Night") to their providing extension IDs (e.g.,
`enkia.tokyo-night`). Cover the themes referenced by the project's
own profile bundles. Unrecognised values are left in place — VS Code
is presumed to resolve them as built-in theme names.

**Rationale**: the marketplace API is rate-limited and requires
network. A static table is fast, deterministic, and sufficient for
the project's own profile output. The table can grow with the
project.

**Alternatives considered**:

- *Query the VS Code marketplace API*: adds a network dependency to
  what is otherwise a local-only script. Rate-limited.
- *Walk every installed extension's `package.json` for
  `contributes.themes`*: works, but hits dozens of files per import
  and only on the user's machine — rules out CI use of the script.
- *Delegate to a `code` CLI subcommand*: `code` has no facility for
  "given a theme name, what extension provides it." Would require
  spawning extension hosts.

### Decision 3: Remove the key, do not substitute a built-in default

**Choice**: when `workbench.colorTheme` references a missing
extension, *delete* the key from the imported settings — do NOT
substitute `"Default Dark Modern"` or any specific built-in.

**Rationale**: the deleted-key fallback lets VS Code apply the
user's existing global preference (which is also a built-in). If
the user has never configured a theme, VS Code defaults to its
own current default. Hard-coding `Default Dark Modern` would
override a user who *did* set a different built-in globally.

**Alternatives considered**:

- *Substitute `Default Dark Modern`*: rejected — overrides any
  user-level preference.
- *Substitute the legacy `Dark+`*: rejected — `Dark+` is kept for
  backwards compatibility; `Default Dark Modern` is the canonical
  modern dark.

### Decision 4: Rewrite happens after the install pass, not interleaved

**Choice**: complete the entire `code --install-extension` loop
first, then run a single rewrite pass with the failed-install list.
Don't decide per-extension as failures occur.

**Rationale**: simpler to reason about. The current script already
captures `FAILED_EXT[@]` and prints them at the end; the rewrite
just slots in before that final report.

**Alternatives considered**:

- *Streaming rewrite as failures occur*: more complex; the order
  matters when multiple keys reference the same extension (icon
  themes especially). One rewrite pass with the full failure list
  avoids ordering bugs.

### Decision 5: Tests live wherever the bats migration sits

**Choice**: write the regression test in whichever test surface is
canonical when this change merges. If `migrate-tests-to-bats`
(PR #3) has merged, write a bats test under
`scripts/tests/bats/import-profile.bats`. Otherwise extend
`scripts/tests/run.sh` and migrate during the bats cutover.

**Rationale**: avoids coupling two changes. Either way the test
exists somewhere and the parity-checkpoint task in the bats
migration ensures it migrates correctly.

**Alternatives considered**:

- *Write the bats test now even if the migration hasn't merged*:
  tempting but creates a dangling test file outside the bats
  scaffold.
- *Block this change on the bats migration*: unnecessarily
  serializes two independent changes.

## Risks / Trade-offs

- **[Label-to-ID map drift]** A new theme is added to the project's
  bundles; its label is not in the map; `import-profile.sh` doesn't
  recognise it on failure. → Mitigation: include a code-level
  comment pointing at the map; PRs that add a new theme to
  `_overrides/` or `_shared/` should add the entry. Not perfect, but
  the failure mode is "key left in place" — same as today's bug,
  which is what we're already fixing for the *known* themes.

- **[`jq` rewrite preserves comments?]** `import-profile.sh` already
  uses `jq` to write `settings.json`; jq strips comments by design.
  But that file is a *cache* under VS Code's user data dir, never
  hand-edited. → Mitigation: nothing to mitigate; comments aren't
  in scope for the cache file.

- **[Failure to delete is silent]** If `jq` fails partway through
  the rewrite, we'd write a half-baked settings.json. → Mitigation:
  use the existing atomic-write pattern from
  `compose-settings.sh:144-150` (write to `.tmp.$$`, mv on
  success).

- **[Other settings keys reference missing extensions too]** Beyond
  themes, settings keys like `terminal.integrated.profiles.linux`
  (custom terminal profile referencing an extension), or any
  `[language]` formatter set to a missing extension's ID, would
  also break. → Out of scope; the bug we're solving is specifically
  about visual theming, which is the user-visible symptom. A
  follow-up could broaden the rewrite to any setting whose value
  is `<publisher>.<name>` — but that requires a much bigger
  inventory and risks over-deletion.

## Migration Plan

This is a single-file behaviour change inside one script. No
external state, no database, no API.

1. Modify `scripts/import-profile.sh` to add the label-to-ID map,
   the post-install rewrite pass, and the atomic write.
2. Modify `scripts/export-profiles.sh` to print the
   `import-profile.sh` notice as the final line.
3. Edit `README.md` Quick Start.
4. Add the regression test to whichever surface is canonical
   (Decision 5).
5. Run the existing test suite (`bash scripts/tests/run.sh`) and
   confirm it still passes plus the new regression test.

**Rollback**: revert the script change. The atomic-write pattern
ensures partial state isn't possible; the worst case is the
behaviour reverts to today's silent-bug status quo.

## Open Questions

- *Should the script print which keys were stripped?* Bias toward
  yes — print to stderr like the existing failure list. This makes
  the silent rewrite visible. Will codify in tasks.md.
- *Should the label-to-ID map live in a separate file?* Bias toward
  no — too small to deserve a file. If it grows past 30 entries,
  reconsider.
