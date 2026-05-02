# Specification: Manage Profile CLI

## Purpose
Define CLI behavior for listing profiles, opening workspaces, and delegating profile workflows.

## Requirements

### Requirement: Profile discovery and listing
The CLI SHALL list profiles by enumerating directories under `profiles/` in sorted order.

#### Scenario: List profiles
- **WHEN** `vspcli --list` runs
- **THEN** it outputs the available profile names in sorted order.

### Requirement: Shell completion output
The CLI SHALL output completion scripts for bash, zsh, or fish that include the current profile list.

#### Scenario: Zsh completion output
- **WHEN** `vspcli --completion zsh` runs
- **THEN** it prints a completion snippet that includes the profile names.

#### Scenario: Unsupported shell
- **WHEN** `vspcli --completion powershell` runs
- **THEN** it exits non-zero with a message about supported shells.

### Requirement: Open profiles in VS Code
The CLI SHALL open a workspace using `code --profile <name> <path>` and require the `code` CLI to be available on PATH.

#### Scenario: Open workspace
- **WHEN** `vspcli --open web-astro-crisp /tmp` runs
- **THEN** it invokes `code --profile web-astro-crisp /tmp`.

#### Scenario: Code CLI missing
- **WHEN** `code` is not available on PATH
- **THEN** the CLI exits non-zero with a clear error.

### Requirement: Install extensions for a profile
The CLI SHALL install extensions by delegating to `scripts/install-extensions.sh` and SHALL support group filters and single-extension installs.

#### Scenario: Install group filters
- **WHEN** `vspcli --install java-profile-crisp --group Java --group AI` runs
- **THEN** it passes the group arguments to `scripts/install-extensions.sh` for that profile.

#### Scenario: Install single extension
- **WHEN** `vspcli --install-ext rust-profile-retina github.copilot` runs
- **THEN** it invokes `code --profile rust-profile-retina --install-extension github.copilot`.

### Requirement: Delegate profile workflows
The CLI SHALL delegate compose, export, open-profiles, and profile-import actions to the corresponding scripts.

#### Scenario: Compose selected profiles
- **WHEN** `vspcli --compose cpp-clangd` runs
- **THEN** it executes `scripts/compose-settings.sh cpp-clangd`.

#### Scenario: Export selected profiles
- **WHEN** `vspcli --export ai-profile-crisp` runs
- **THEN** it executes `scripts/export-profiles.sh ai-profile-crisp`.

#### Scenario: Open profiles without installing extensions
- **WHEN** `vspcli --open-profiles --skip-install` runs
- **THEN** it sets `VSCODE_SKIP_EXTENSION_INSTALL=1` when invoking `scripts/open-profiles.sh`.

#### Scenario: Import bundle
- **WHEN** `vspcli --profile-import rust-profile-retina exports/rust-profile-retina.code-profile` runs
- **THEN** it executes `scripts/import-profile.sh` with those arguments.
