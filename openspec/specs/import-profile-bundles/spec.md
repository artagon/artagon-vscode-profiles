# Specification: Import Profile Bundles

## Purpose
Define how `.code-profile` bundles are imported into VS Code profile storage.

## Requirements

### Requirement: Inputs and dependencies
The system SHALL require a `.code-profile` file path, `jq`, and the VS Code `code` CLI on PATH, and exit non-zero with clear errors when any are missing.

#### Scenario: Missing bundle path
- **WHEN** `import-profile.sh` runs without a valid bundle file
- **THEN** it exits non-zero with a usage message or error.

#### Scenario: Dependencies missing
- **WHEN** `jq` or `code` is not available on PATH
- **THEN** the script exits non-zero with an error message.

### Requirement: User data directory detection
The system SHALL resolve the VS Code user data directory from `VSCODE_USER_DIR` when set, otherwise use OS defaults.

#### Scenario: Override directory
- **WHEN** `VSCODE_USER_DIR` is set
- **THEN** the script uses that path for profile storage.

### Requirement: Profile registration
The system SHALL ensure `globalStorage/storage.json` exists and contains a `userDataProfiles` entry for the named profile.

#### Scenario: Existing profile entry
- **WHEN** the profile already exists in `storage.json`
- **THEN** the existing profile ID is reused.

#### Scenario: New profile entry
- **WHEN** the profile does not exist in `storage.json`
- **THEN** a new profile entry is created with a generated ID.

### Requirement: Settings import
The system SHALL write the bundle's `settings` object (or `{}` when absent) into the profile's cached `settings.json`.

#### Scenario: Settings imported
- **WHEN** the bundle contains a `settings` object
- **THEN** the profile cache receives those settings.

### Requirement: Extension installation
The system SHALL install extensions listed in `extensions.enabled` using `code --profile <name> --install-extension` and continue even if an extension install fails.

#### Scenario: Extensions list missing
- **WHEN** the bundle has no `extensions.enabled` list
- **THEN** the script skips extension installation.

#### Scenario: Extension install failure
- **WHEN** an extension install fails
- **THEN** the script continues with the remaining extensions.
