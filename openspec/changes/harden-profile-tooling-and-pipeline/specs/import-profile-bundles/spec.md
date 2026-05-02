## ADDED Requirements

### Requirement: Robust profile ID generation
Profile-ID generation SHALL succeed under `set -euo pipefail` and SHALL produce an 8-character lowercase hexadecimal value.

#### Scenario: Generation under strict shell options
- **WHEN** `import-profile.sh` runs under `set -euo pipefail`
- **THEN** PROFILE_ID generation completes without aborting and yields an 8-character lowercase hexadecimal value

### Requirement: Imported extension ID validation
The importer SHALL validate every identifier in the bundle's `extensions.enabled` list against the marketplace ID pattern `^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$` before invoking `code --install-extension`, and SHALL reject the import with a non-zero exit when any identifier fails the pattern.

#### Scenario: Bundle contains a malformed identifier
- **WHEN** an imported `.code-profile` bundle declares an `extensions.enabled` entry that is not a valid marketplace identifier
- **THEN** the importer reports the offending value and exits non-zero before any extension is installed

## MODIFIED Requirements

### Requirement: Extension installation
The system SHALL validate identifiers from `extensions.enabled` against the marketplace ID pattern, install matching extensions using `code --profile <name> --install-extension`, and SHALL surface a non-zero exit code when any individual install fails.

#### Scenario: Extensions list missing
- **WHEN** the bundle has no `extensions.enabled` list
- **THEN** the script skips extension installation

#### Scenario: Extension install failure
- **WHEN** an extension install returns a non-zero status
- **THEN** the script records the failure, continues with the remaining extensions, and exits non-zero after the loop completes
