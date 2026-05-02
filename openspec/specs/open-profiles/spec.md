# Specification: Open Profiles

## Purpose
Define how profile windows are opened, extensions are installed, and cached settings are synced.

## Requirements

### Requirement: Profile selection
The system SHALL open only the profiles named on the command line, or all profiles under `profiles/` when none are provided.

#### Scenario: Explicit profile list
- **WHEN** `open-profiles.sh` runs with `web-astro-crisp rust-profile-retina`
- **THEN** it opens only those profiles.

#### Scenario: No profiles provided
- **WHEN** `open-profiles.sh` runs with no profile arguments
- **THEN** it discovers profile directories under `profiles/` and opens each.

### Requirement: VS Code CLI dependency
The system SHALL require the `code` CLI on PATH and exit with a clear error when it is missing.

#### Scenario: CLI missing
- **WHEN** `code` is not available on PATH
- **THEN** the script exits non-zero with an error message.

### Requirement: Skip extension installation
The system SHALL skip extension installation when `--skip-install` is passed or `VSCODE_SKIP_EXTENSION_INSTALL=1` is set.

#### Scenario: Skip install flag
- **WHEN** `open-profiles.sh --skip-install` runs
- **THEN** it does not invoke `scripts/install-extensions.sh`.

### Requirement: Extension installation caching
The system SHALL cache extension installs per profile under `.cache/extensions-installed` using a hash of `profiles/<name>/extensions.json`.

#### Scenario: Cache hit
- **WHEN** the cached hash matches the current extensions list
- **THEN** the script does not reinstall extensions for that profile.

### Requirement: Profile cache syncing
The system SHALL attempt to sync `profiles/<name>/settings.json` into the VS Code user profile cache when `globalStorage/storage.json` contains a matching profile entry.

#### Scenario: Profile is registered
- **WHEN** `storage.json` contains a `userDataProfiles` entry for `web-astro-crisp`
- **THEN** the script copies settings to `profiles/<id>/settings.json` under the VS Code user data directory.

#### Scenario: Profile is not registered
- **WHEN** `storage.json` lacks a matching entry
- **THEN** the script warns and continues without syncing.

### Requirement: User data directory override
The system SHALL honor `VSCODE_USER_DIR` as the base for VS Code user data when syncing cached settings.

#### Scenario: Override directory
- **WHEN** `VSCODE_USER_DIR` is set
- **THEN** the script uses that path instead of OS defaults.

### Requirement: jq dependency for cache syncing
The system SHALL require `jq` for profile cache syncing and exit with a clear error when it is missing.

#### Scenario: jq missing
- **WHEN** `jq` is not available on PATH
- **THEN** the script exits non-zero with an error message.
