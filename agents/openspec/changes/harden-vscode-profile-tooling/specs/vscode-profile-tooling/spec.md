# Specification: VS Code Profile Tooling

## ADDED Requirements

### Requirement: Hermetic Script Tests
The system SHALL run script smoke tests without modifying the user's real VS Code profile cache or configuration directories.

#### Scenario: Tests isolate VS Code user data
- **WHEN** scripts/tests/run.sh executes
- **THEN** it SHALL set VSCODE_USER_DIR to a temporary path before calling open-profiles.sh or import-profile.sh
- **AND** any writes to globalStorage or profiles SHALL stay within that temporary path

### Requirement: Resilient Temporary Files
The system SHALL create temporary validation files even when TMPDIR is unset.

#### Scenario: TMPDIR is unset
- **WHEN** validate-json.sh runs and TMPDIR is not set
- **THEN** it SHALL fall back to a system temp directory (for example, /tmp)
- **AND** JSON validation SHALL proceed without unbound variable errors

### Requirement: Reliable Profile ID Generation
The system SHALL generate profile IDs in import-profile.sh without pipefail-related termination.

#### Scenario: Pipefail environment
- **WHEN** import-profile.sh runs under set -euo pipefail
- **THEN** profile ID generation SHALL succeed without SIGPIPE
- **AND** the profile cache entry SHALL be created

### Requirement: Portable C/C++ Toolchain Settings
The system SHALL avoid hard-coded toolchain paths that only work on Homebrew or one architecture.

#### Scenario: Non-Homebrew host
- **WHEN** profiles are used on a host without /opt/homebrew
- **THEN** C/C++ settings SHALL rely on PATH or documented overrides
- **AND** clangd, cmake, and clang-format configuration SHALL not reference missing absolute paths

#### Scenario: Documented overrides
- **WHEN** a user configures toolchain overrides as documented
- **THEN** the profiles SHALL use those overrides for clangd, clang-format, and cmake

### Requirement: Extension Group Coverage
The system SHALL include all AI extensions used in ai-profile and ai-plus when filtering by AI group.

#### Scenario: AI group installation
- **WHEN** install-extensions.sh is executed with --group AI
- **THEN** it SHALL include github.copilot, github.copilot-chat, sourcegraph.cody-ai, and continue.continue (when present in the profile)

### Requirement: Documentation-Behavior Alignment
The system SHALL keep documentation consistent with actual profile contents and script behavior.

#### Scenario: AI policy documentation is accurate
- **WHEN** README.md and agents docs describe AI assistant policy
- **THEN** they SHALL match the current ai-profile and ai-plus extension sets

#### Scenario: CLI support documentation is accurate
- **WHEN** documentation references open-profiles.sh CLI support
- **THEN** it SHALL match the script behavior (code vs code-insiders)

### Requirement: Extension Compatibility Checking
The system SHALL provide automated checking of extension compatibility against the current VS Code version.

#### Scenario: Check single profile for incompatible extensions
- **WHEN** check-extension-compatibility.sh is executed with a profile name
- **THEN** it SHALL query the VS Code Marketplace API for each extension in the profile
- **AND** it SHALL extract the engines.vscode requirement from extension metadata
- **AND** it SHALL compare the requirement against the current VS Code version
- **AND** it SHALL report extensions as "compatible", "incompatible", or "unknown"

#### Scenario: Check all profiles
- **WHEN** check-extension-compatibility.sh is executed with --all flag
- **THEN** it SHALL check all profiles found in profiles/*/extensions.json
- **AND** it SHALL provide a summary of total, compatible, incompatible, and unknown extensions

#### Scenario: Cache Marketplace API responses
- **WHEN** check-extension-compatibility.sh queries extension metadata
- **THEN** it SHALL cache responses in ~/.cache/vscode-extension-check/
- **AND** cached responses SHALL have a TTL of 1 hour
- **AND** the --no-cache flag SHALL bypass the cache and fetch fresh data

#### Scenario: JSON output for automation
- **WHEN** check-extension-compatibility.sh is executed with --json flag
- **THEN** it SHALL output structured JSON including vscodeVersion, summary, and extensions array
- **AND** each extension entry SHALL include profile, extension ID, status, latest version, engine requirement, installed version, and last updated timestamp

#### Scenario: Verbose output shows all extensions
- **WHEN** check-extension-compatibility.sh is executed with --verbose flag
- **THEN** it SHALL display all extensions including compatible ones
- **AND** it SHALL show version and engine requirement for each extension

#### Scenario: Marketplace-only mode for faster checks
- **WHEN** check-extension-compatibility.sh is executed with --marketplace-only flag
- **THEN** it SHALL skip checking if extensions are installed locally
- **AND** installed version SHALL be reported as "not installed"

#### Scenario: Graceful handling of API failures
- **WHEN** Marketplace API is unavailable or rate-limited
- **THEN** the script SHALL report affected extensions with "unknown" status
- **AND** it SHALL continue checking remaining extensions
- **AND** it SHALL provide a warning message about API issues
