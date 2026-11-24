# Delta Specification: Open Profiles

## MODIFIED Requirements

### Requirement: Extension installation caching

The system SHALL cache extension installs per profile under `.cache/extensions-installed` keyed by a combined hash of `profiles/<name>/extensions.json`, `catalog/extension-pins.json`, and `CODE_USER_DIR`, pass consistent `--user-data-dir` to `code`, and propagate non-zero failures from `install-extensions.sh` without recording a cache hit.

#### Scenario: Cache hit
- **WHEN** the cached combined hash matches the current extensions list, pins manifest, and `CODE_USER_DIR`
- **THEN** the script does not reinstall extensions for that profile.

#### Scenario: Updating extension pins invalidates install cache and failures are not cached
- **WHEN** `catalog/extension-pins.json` changes or `install-extensions.sh` exits non-zero
- **THEN** the cache is invalidated on pin changes and failed installs exit non-zero without writing a cache marker.

### Requirement: Profile cache syncing

The system SHALL sync `_merged/<name>.json` into `<CODE_USER_DIR>/profiles/<id>/settings.json` (`0600`, with timestamped `.bak` backup) when `globalStorage/storage.json` contains a matching entry, preserving existing local 14-key UX settings, and SHALL pass consistent `--user-data-dir` and `--` before workspace paths when launching `code`.

#### Scenario: Profile is registered
- **WHEN** `storage.json` contains a `userDataProfiles` entry for `rust` with local `workbench.colorTheme: "Catppuccin Mocha"`
- **THEN** the script backs up and syncs `_merged/rust.json` to `profiles/<id>/settings.json` (`0600`) while preserving `workbench.colorTheme: "Catppuccin Mocha"`.

#### Scenario: Profile is not registered
- **WHEN** `storage.json` lacks a matching entry
- **THEN** the script warns and continues without syncing.
