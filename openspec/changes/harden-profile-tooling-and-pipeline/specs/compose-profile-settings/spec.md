## ADDED Requirements

### Requirement: Crash-safe merge output
The composer SHALL produce `_merged/<name>.json` such that readers (including VS Code via the profile symlink) never observe a truncated, empty, or partially-written file, even if the composer is interrupted or its underlying tools fail mid-write.

#### Scenario: Merge interrupted mid-write
- **WHEN** `compose-settings.sh` is interrupted while producing `_merged/<name>.json`
- **THEN** the previous `_merged/<name>.json` remains intact and no partial file is exposed at the final path

#### Scenario: Underlying tool fails during merge
- **WHEN** the underlying merge tool exits non-zero while producing the merged output
- **THEN** the existing `_merged/<name>.json` is preserved and the script exits non-zero without overwriting it

### Requirement: Strict @extends parsing
The composer SHALL surface parse errors when reading `@extends` declarations and SHALL NOT silently treat malformed values as empty.

#### Scenario: Malformed @extends value
- **WHEN** an override declares a non-string, non-array `@extends` (for example `"@extends": 1`)
- **THEN** the composer reports the parse error and exits non-zero for that profile

### Requirement: Path containment for @extends
The composer SHALL accept only `@extends` values whose lexical shape is `<segment>(/<segment>)?\.jsonc` (a bare filename or a single sub-directory plus filename) where each segment matches `[a-zA-Z0-9._-]+`, AND whose resolved real path lies within the real path of `_overrides/`. Any other value (including `..` segments, leading `/`, leading `~`, backslashes, NUL bytes, multi-level sub-paths, or values that resolve outside `_overrides/`) SHALL be rejected with a clear error.

#### Scenario: Traversal segment
- **WHEN** an override declares `"@extends": ["../../../etc/passwd"]`
- **THEN** the composer reports the rejected path and exits non-zero for that profile

#### Scenario: Absolute path
- **WHEN** an override declares `"@extends": ["/tmp/evil.jsonc"]`
- **THEN** the composer reports the rejected path and exits non-zero for that profile

#### Scenario: Home expansion
- **WHEN** an override declares `"@extends": ["~/secret.jsonc"]`
- **THEN** the composer reports the rejected path and exits non-zero for that profile

### Requirement: Cycle detection by resolved real path
The composer SHALL identify nodes in the `@extends` graph by their resolved real path (after symlink resolution) so that cycle detection works regardless of whether overrides reach a target via symlink or direct filename.

#### Scenario: Self-cycle through symlink
- **WHEN** an override is a symlink whose target's `@extends` chain leads back to the same real path
- **THEN** the composer reports a cycle and exits non-zero for that profile

## MODIFIED Requirements

### Requirement: Override chain resolution
The system SHALL resolve `@extends` references in `_overrides/<profile>.jsonc` in parent-first order, remove `@extends` before merging, surface parse errors, and detect cycles by resolved real path.

#### Scenario: Parent overrides apply before child
- **WHEN** an override declares `"@extends": ["java-profile-base.jsonc", "java-spring-base.jsonc"]`
- **THEN** the composer merges the shared base, then the parent overrides, then the profile override

#### Scenario: Missing parent override
- **WHEN** a referenced parent file does not exist
- **THEN** the composer reports a warning and continues with remaining overrides

#### Scenario: Cycle in @extends chain
- **WHEN** the resolved real-path chain contains a cycle
- **THEN** the composer reports the cycle path and exits non-zero
