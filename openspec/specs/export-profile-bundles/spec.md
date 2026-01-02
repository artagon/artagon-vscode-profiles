# Specification: Export Profile Bundles

## Purpose
Define how `.code-profile` bundles are generated from merged settings and extension lists.

## Requirements

### Requirement: Export bundle per profile
The system SHALL create `exports/<profile>.code-profile` for each profile directory that has `_merged/<profile>.json` and `profiles/<profile>/extensions.json`.

#### Scenario: Export includes settings and extensions
- **WHEN** export runs for `rust-profile-crisp` and both source files exist
- **THEN** the bundle contains `settings` from `_merged/rust-profile-crisp.json`
- **AND** `extensions.enabled` is a list of extension IDs from `profiles/rust-profile-crisp/extensions.json`.

### Requirement: Skip missing inputs
The system SHALL skip profiles that are missing the merged settings file or the extensions list and report a warning.

#### Scenario: Missing merged settings
- **WHEN** `_merged/<profile>.json` is missing
- **THEN** the exporter skips that profile and reports a warning.

#### Scenario: Missing extensions list
- **WHEN** `profiles/<profile>/extensions.json` is missing
- **THEN** the exporter skips that profile and reports a warning.

### Requirement: Exports directory creation
The system SHALL create the `exports/` directory if it does not exist.

#### Scenario: Exports directory missing
- **WHEN** export runs and `exports/` is absent
- **THEN** the exporter creates `exports/` before writing bundles.
