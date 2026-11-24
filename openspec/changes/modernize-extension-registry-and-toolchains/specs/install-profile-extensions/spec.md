## ADDED Requirements

### Requirement: Modern Publisher Allowlist
The system SHALL validate all installed extension identifiers against an updated publisher allowlist in `scripts/lib/extension-id.sh` including `mads-hartmann`, `charliermarsh`, `ms-pyright`, `astral-sh`, `bmalehorn`, `jeff-hykin`, `jetmartin`, and `fill-labs`.

#### Scenario: Installing allowlisted modern extension
- **WHEN** an extension from a newly added modern publisher is installed
- **THEN** identifier validation succeeds without requiring manual bypass.

## MODIFIED Requirements

### Requirement: Extension grouping
The system SHALL categorize extensions into `AI`, `CMake`, `Java`, `Rust`, `Shell`, `Python`, or `General` groups and default to `General` when no mapping exists.

#### Scenario: Group filter
- **WHEN** `--group Shell --group Python` is provided
- **THEN** only extensions in those groups are installed.

#### Scenario: Unknown group
- **WHEN** an unknown group is provided
- **THEN** the script warns and skips that group.
