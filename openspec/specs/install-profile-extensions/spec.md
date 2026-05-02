# Specification: Install Profile Extensions

## Purpose
Define how profile extension lists are installed and filtered by group.

## Requirements

### Requirement: Dependencies
The system SHALL require `jq` and the VS Code `code` CLI on PATH and exit with clear errors when either is missing.

#### Scenario: jq missing
- **WHEN** `install-extensions.sh` runs without `jq`
- **THEN** it exits non-zero with an error message.

#### Scenario: code CLI missing
- **WHEN** `install-extensions.sh` runs without `code` on PATH
- **THEN** it exits non-zero with an error message.

### Requirement: Profile selection
The system SHALL install extensions from `profiles/<profile>/extensions.json` and exit non-zero when the file is missing.

#### Scenario: Extensions file missing
- **WHEN** the profile extensions file does not exist
- **THEN** the script exits non-zero and reports the missing path.

#### Scenario: All profiles
- **WHEN** the profile name is `all`
- **THEN** the script installs extensions for every profile under `profiles/` and exits non-zero if any profile fails.

### Requirement: Extension ID extraction
The system SHALL read extension IDs from `.identifier.id` entries in `profiles/<profile>/extensions.json`.

#### Scenario: Empty extension list
- **WHEN** no extension IDs are present
- **THEN** the script exits successfully without installing anything.

### Requirement: Extension grouping
The system SHALL categorize extensions into `AI`, `CMake`, `Java`, `Rust`, or `General` groups and default to `General` when no mapping exists.

#### Scenario: Group filter
- **WHEN** `--group AI --group Java` is provided
- **THEN** only extensions in those groups are installed.

#### Scenario: Unknown group
- **WHEN** an unknown group is provided
- **THEN** the script warns and skips that group.

### Requirement: Installation order and delay
The system SHALL install extensions in group order `AI`, `CMake`, `Java`, `Rust`, `General` and pause between installs using `VSCODE_EXTENSION_INSTALL_DELAY` (default 1 second).

#### Scenario: Group ordering
- **WHEN** multiple groups are installed
- **THEN** extensions are installed in the defined group order.

### Requirement: Failure reporting
The system SHALL report failed installs and exit non-zero when any extension fails to install.

#### Scenario: Extension install failure
- **WHEN** an extension install returns a non-zero status
- **THEN** the script lists the failed IDs and exits non-zero.
