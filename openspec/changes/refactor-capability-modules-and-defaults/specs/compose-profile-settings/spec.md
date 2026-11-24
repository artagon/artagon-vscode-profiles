# Delta Specification: Compose Profile Settings

## REMOVED Requirements

### Requirement: Base selection by profile name
**Reason**: Legacy `-crisp`/`-retina` filename-based base selection is replaced by recipe-driven vertical module composition and orthogonal 14-key UX presets (`modules/ux/{crisp,retina,default}/`).
**Migration**: Profile recipes under `profiles/<name>/profile.json` compose `catalog/defaults.json` plus explicit `modules[]`. Legacy names resolve via `scripts/lib/legacy-profile-name.sh`.

### Requirement: Override chain resolution
**Reason**: Legacy `_overrides/<profile>.jsonc` `@extends` inheritance chains are retired and replaced by vertical module `requires[]` DAG resolution under `modules/`.
**Migration**: Express parent dependencies via `requires[]` in `modules/*/*/module.json` and profile membership in `profiles/<name>/profile.json`.

### Requirement: Skip incomplete profiles
**Reason**: `_overrides/<profile>.jsonc` no longer exists; profile completeness is determined by `profiles/<name>/profile.json`.
**Migration**: Missing or invalid `profiles/<name>/profile.json` fails validation explicitly rather than silently skipping.

## MODIFIED Requirements

### Requirement: Merge output and symlink

The system SHALL write UX-agnostic merged settings (`ux: "default"`) atomically to `_merged/<profile>.json` (`0644`), refresh `profiles/<profile>/settings.json` as a repo-relative symlink to `../../_merged/<profile>.json`, and atomically regenerate `profiles/<profile>/extensions.json` (`0644`).

#### Scenario: Merge output is created
- **WHEN** compose completes for `astro`
- **THEN** `_merged/astro.json` (`0644`) exists with UX-agnostic merged settings omitting all 14 symmetric UX keys.

#### Scenario: Symlink is refreshed
- **WHEN** compose completes for `astro`
- **THEN** `profiles/astro/settings.json` points to `../../_merged/astro.json` and `profiles/astro/extensions.json` (`0644`) is regenerated from the resolved module DAG.

## ADDED Requirements

### Requirement: Symmetric 14-key UX preset separation and verified icon themes

`modules/ux/crisp/settings.profile.jsonc` and `modules/ux/retina/settings.profile.jsonc` SHALL define identical 14-key visual and typography presets (using verified `catppuccin-mocha` icon theme for `retina`, `editor.lineHeight` multipliers `1.6`/`1.65`, and Nerd Font terminal fallbacks) completely disjoint from `modules/capabilities/core-editor/`, while `modules/ux/default/settings.profile.jsonc` SHALL be `{}` and delete all 14 keys on explicit apply.

#### Scenario: UX symmetry gate verifies exact 14 keys and retina-to-default resets cleanly
- **WHEN** `scripts/tests/check-ux-symmetry.sh` runs and a Retina-styled settings file is updated with `--ux=default`
- **THEN** `crisp` and `retina` match the exact 14 managed keys disjoint from `core-editor`, `retina` uses `catppuccin-mocha`, `default` is `{}`, and `--ux=default` deletes all 14 keys while preserving unrelated user preferences.

### Requirement: Portable XDG RTK terminal initialization and plain workspace tasks

`modules/capabilities/rtk-terminal/scripts/rtk-init.{fish,bash,zsh}` SHALL install via `--no-follow` atomic writes into a validated absolute XDG dir, split Bash RC-load (`_ARTAGON_RTK_RC_LOADED`) from `ARTAGON_RTK_WRAPPED`, unalias wrapped commands, preserve/restore `ARTAGON_ORIG_ZDOTDIR` in Zsh shims, quote Fish space paths, and emit plain `cargo doc` tasks in portable workspaces (`rtk` only with `--rtk-in-workspace` or profile scope).

#### Scenario: Bash wrapper prevents recursion, survives grep alias, and returns 0 non-interactively
- **WHEN** `~/.bashrc` sources `rtk-init.bash` with `alias grep='grep --color=auto'` in a non-interactive Bash 3.2 shell
- **THEN** `_ARTAGON_RTK_RC_LOADED` prevents infinite recursion, `grep` wraps without syntax error, and the script returns status `0`.

#### Scenario: Fish wrapper leaves guard unset without rtk and handles spaced XDG paths
- **WHEN** `rtk-init.fish` is sourced without `rtk` on PATH and then re-sourced with `rtk` present under an XDG path containing spaces
- **THEN** the first source returns `0` with `ARTAGON_RTK_WRAPPED` unset, the second source sets `set -g ARTAGON_RTK_WRAPPED 1`, and interactive output prints at most once.

#### Scenario: Zsh ZDOTDIR proxy preserves custom ZDOTDIR and default workspace tasks omit rtk
- **WHEN** Zsh launches through the RTK proxy with a custom original `ZDOTDIR`, and `vspcli apply --target=workspace` runs on a Rust repo without `--rtk-in-workspace`
- **THEN** Zsh chains the original `ZDOTDIR` startup files plus `rtk-init.zsh` and restores `ZDOTDIR`, while `.vscode/tasks.json` uses plain `cargo doc` commands.
