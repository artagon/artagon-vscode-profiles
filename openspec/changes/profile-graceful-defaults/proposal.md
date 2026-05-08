## Why

Generated `.code-profile` bundles list theme and icon-theme extensions in
`extensions.enabled`, but VS Code's UI profile import (Settings → Profiles →
Import) only **enables** what is already installed — it does not install
missing extensions. Users who import that way silently fall through to
whatever theme name resolves to (often a default workbench theme without the
styled palette) and broken icon glyphs. The same profile imported via
`scripts/import-profile.sh` looks correct because that script installs every
listed extension before VS Code reads the settings.

Net effect: a fresh user comparing their existing default profile (which uses
the always-present built-in `Dark+`) to a generated profile (which references
`Tokyo Night` or `Catppuccin Mocha` extensions they never installed) sees the
generated profile as visibly worse — broken theme, broken icons — when in
fact the generated profile is fine but the import path silently degraded it.

## What Changes

- `scripts/import-profile.sh` learns to detect failed extension installs,
  identify whether any failed entries are workbench color-themes or
  icon-themes, and rewrite the imported profile's `settings.json` to drop
  `workbench.colorTheme` and/or `workbench.iconTheme` whose target extensions
  did not install. VS Code falls back to its built-in default theme instead
  of "broken theme name" → silent default.
- `scripts/export-profiles.sh` prints a final-line notice telling whoever
  built the bundle that recipients should import via `import-profile.sh`,
  not via VS Code's UI.
- `README.md` Quick Start moves `import-profile.sh` above the UI-import
  path, with a one-line warning that UI import does not install missing
  extensions.
- A regression test asserts that when an extension's install is simulated
  as failed, the post-import settings file no longer references that
  extension's theme keys. **Test placement depends on
  `migrate-tests-to-bats` (PR #3) sequencing**: if that change has merged
  by the time this one lands, the test is written as a bats test under
  `scripts/tests/bats/import-profile.bats`. Otherwise it's added to
  `scripts/tests/run.sh` and migrated as part of the bats cutover.

No changes to `_shared/`, `_overrides/`, or any of the 22 profile manifests.
The styled-theme look is preserved for users on the canonical install path.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `import-profile-bundles`: graceful fallback when theme/icon-theme
  extensions fail to install — strip the dangling theme keys from the
  imported settings so the workbench loads VS Code's built-in default
  theme rather than silently failing to apply a missing theme.

## Impact

- **Code**: `scripts/import-profile.sh`, `scripts/export-profiles.sh`,
  `README.md`, plus the test surface chosen by the bats-migration
  sequencing (`scripts/tests/run.sh` or `scripts/tests/bats/import-profile.bats`).
- **APIs**: `import-profile.sh` CLI surface unchanged; new internal logic
  only.
- **Dependencies**: existing (`jq`, `code` CLI). No new deps.
- **Backwards compatibility**: fully backwards compatible. Profiles that
  import successfully (all extensions install) behave identically to today.
  The new behavior only triggers on install failure, replacing a silent
  visual bug with a deterministic fallback.
- **Risk**: low. Change is localized to one script's post-install pass.
