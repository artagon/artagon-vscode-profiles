# Specification: Compose Profile Settings

## Purpose
Describe how shared bases and profile overrides are merged into `_merged/` and linked into `profiles/`.

## Requirements

### Requirement: Base selection by profile name
The system SHALL select the shared base settings file based on the profile name: `_shared/editor-retina.jsonc` when the name contains `retina`, otherwise `_shared/editor-crisp.jsonc`.

#### Scenario: Retina profile uses retina base
- **WHEN** compose runs for `rust-profile-retina`
- **THEN** it uses `_shared/editor-retina.jsonc` as the base.

#### Scenario: Crisp profile uses crisp base
- **WHEN** compose runs for `rust-profile-crisp`
- **THEN** it uses `_shared/editor-crisp.jsonc` as the base.

### Requirement: Override chain resolution
The system SHALL resolve `@extends` references in `_overrides/<profile>.jsonc` in parent-first order and remove `@extends` before merging.

#### Scenario: Parent overrides apply before child
- **WHEN** an override declares `"@extends": ["java-profile-base.jsonc", "java-spring-base.jsonc"]`
- **THEN** the composer merges the shared base, then the parent overrides, then the profile override.

#### Scenario: Missing parent override
- **WHEN** a referenced parent file does not exist
- **THEN** the composer reports a warning and continues with remaining overrides.

### Requirement: Merge output and symlink
The system SHALL write merged settings to `_merged/<profile>.json` and refresh `profiles/<profile>/settings.json` as a repo-relative symlink to `../../_merged/<profile>.json`.

#### Scenario: Merge output is created
- **WHEN** compose completes for `web-astro-crisp`
- **THEN** `_merged/web-astro-crisp.json` exists with merged settings.

#### Scenario: Symlink is refreshed
- **WHEN** compose completes for `web-astro-crisp`
- **THEN** `profiles/web-astro-crisp/settings.json` points to `../../_merged/web-astro-crisp.json`.

### Requirement: Skip incomplete profiles
The system SHALL skip composition when the override file or profile directory is missing.

#### Scenario: Missing override file
- **WHEN** `_overrides/<profile>.jsonc` is absent
- **THEN** the composer skips the profile without writing merged output.

#### Scenario: Missing profile directory
- **WHEN** `profiles/<profile>/` does not exist
- **THEN** the composer skips the profile without writing merged output.
