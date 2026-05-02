# Specification: Validate Profile JSON

## Purpose
Define validation expectations for JSON and JSONC fragments used by profiles.

## Requirements

### Requirement: Validate shared, override, and extension files
The system SHALL validate JSON and JSONC files under `_shared/`, `_overrides/`, and `profiles/*/extensions.json` using `jq`.

#### Scenario: Validation succeeds
- **WHEN** all discovered files parse as JSON
- **THEN** the validator exits successfully and reports the number of files checked.

#### Scenario: Validation fails
- **WHEN** any file fails to parse
- **THEN** the validator reports the failing file path and exits with a non-zero status.

### Requirement: jq dependency
The system SHALL fail fast with a clear error when `jq` is not available on PATH.

#### Scenario: jq is missing
- **WHEN** validation runs without `jq` installed
- **THEN** the validator exits non-zero and reports that `jq` is required.

### Requirement: Empty file set
The system SHALL exit successfully when no JSON or JSONC files are found.

#### Scenario: No JSON files present
- **WHEN** the validator finds no matching files
- **THEN** it exits successfully and reports that there are no files to validate.
