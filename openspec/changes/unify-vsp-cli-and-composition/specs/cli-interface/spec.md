# CLI Interface Delta

## Purpose

Define the native command surface, explicit targeting rules, and v2 compatibility behavior for profile and workspace management.

## ADDED Requirements

### Requirement: Explicit synchronization targets

Native vsp SHALL require an explicit target for sync and diff. Both commands SHALL accept a workspace path, a named profile, a preset, or the Default user target. Sync SHALL also accept bundle import and all-profile remediation targets.

#### Scenario: Missing selector
- **WHEN** a user invokes `vsp sync` or `vsp diff` without a selector
- **THEN** vsp exits `4` and leaves the working directory and VS Code state unchanged.

### Requirement: Read-only detection

`vsp detect` SHALL report matching workspace toolchains without writing workspace or profile files.

#### Scenario: Rust workspace detection
- **GIVEN** a workspace with `Cargo.toml` and no `.vscode/` directory
- **WHEN** a user runs `vsp detect PATH`
- **THEN** vsp reports Rust, exits `0`, and leaves `.vscode/` absent.

### Requirement: Shared preview

`vsp diff` and `vsp sync --dry-run` SHALL emit the same `vsp-plan/v1` plan for identical standard workspace, profile, preset, or user selectors and composition flags. Neither preview SHALL change the target.

#### Scenario: Compare machine-readable plans
- **WHEN** a user runs both previews with `--json` against the same unchanged workspace and preset
- **THEN** the JSON outputs are identical.

### Requirement: Native exit codes

Native vsp SHALL distinguish success (`0`), operational failure (`1`), missing target (`2`), complete negative check (`3`), usage or config error (`4`), safety refusal (`5`), retryable lock or state conflict or partial apply (`6`), and incomplete observation (`7`).

#### Scenario: Automation detects confirmed drift
- **GIVEN** a valid target with settings drift
- **WHEN** a user runs `vsp diff --check` with its selector
- **THEN** vsp reports drift and exits `3`.

#### Scenario: Doctor cannot enumerate extensions
- **WHEN** a required VS Code extension listing fails during native `vsp doctor`
- **THEN** the audit exits `7`.

### Requirement: Legacy parser selection

The vsp binary SHALL use legacy argument parsing when the basename of `argv[0]` is `vspcli` or case-insensitively matches `vspcli.exe`, when `VSP_COMPAT_VSPCLI` is exactly `1`, or when its first argument is `compat-vspcli`.

#### Scenario: Each compatibility entry point accepts legacy list
- **WHEN** a user runs `vspcli --list` via the native binary's link, `VSP_COMPAT_VSPCLI=1 vsp --list`, or `vsp compat-vspcli --list`
- **THEN** each invocation lists presets and exits `0`.

### Requirement: Legacy failure mapping

Compatibility mode SHALL map exits `5`, `6`, and `7` to `1` for legacy `doctor`, `reconcile`, and `install-ext`. Compatibility mode SHALL pass through all other exits.

#### Scenario: Legacy doctor reports observation failure
- **GIVEN** VS Code extension listing fails during an audit
- **WHEN** a user runs `vsp compat-vspcli doctor`
- **THEN** it reports the failure and exits `1`.

#### Scenario: Legacy detect keeps a missing-target exit
- **WHEN** a user runs `vsp compat-vspcli detect PATH` for a missing path
- **THEN** it exits `2`.
