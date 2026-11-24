# Delta Specification: Import Profile Bundles

## MODIFIED Requirements

### Requirement: Profile registration

The system SHALL validate bundle integrity, single-object schema, extension IDs, active publishers, and parsed `.settings` autonomy deny-list BEFORE mutating `storage.json`. Registration SHALL fail if VS Code runs unless `--force`, hold an ABA-free lock with pre-rename SHA-256 CAS, reject duplicates or bad locations, reserve an unused ID, keep bounded `.bak` backups, and write `0600`.

#### Scenario: Existing profile entry
- **WHEN** the profile already exists uniquely with a valid location in `storage.json` and bundle validation passes
- **THEN** the existing profile ID is reused under lock with a timestamped backup written first.

#### Scenario: New profile entry
- **WHEN** the profile does not exist in `storage.json` and bundle validation passes
- **THEN** a new unused 8-hex profile ID is reserved and written atomically (`0600`).

#### Scenario: Poisoned bundle or concurrent VS Code leaves storage.json untouched
- **WHEN** `scripts/import-profile.sh` receives an unknown publisher, invalid ID, non-object `.settings`, non-array `.extensions.enabled`, autonomy deny-list key in `.settings` (without `--allow-unsafe-settings`), duplicate profile entry, or detects VS Code running without `--force`
- **THEN** it exits non-zero before modifying `<CODE_USER_DIR>/globalStorage/storage.json` or creating `<CODE_USER_DIR>/profiles/<id>/settings.json`.

### Requirement: Settings import

The system SHALL write the bundle's `settings` (`0600`, bounded `.bak` backup) and, on failed theme/icon install, match failed IDs case-insensitively against `catalog/extensions.json` `themeLabels` and strip `workbench.colorTheme` + `workbench.preferredDarkColorTheme` (color) or `workbench.iconTheme` (icon), preserving unrecognized themes.

#### Scenario: Settings imported
- **WHEN** the bundle contains a valid `settings` object and all extensions succeed
- **THEN** the profile cache receives byte-equivalent settings (`0600`).

#### Scenario: Failed color theme strips both colorTheme and preferredDarkColorTheme
- **WHEN** `enkia.tokyo-night` fails to install during import of a bundle setting both `workbench.colorTheme` and `workbench.preferredDarkColorTheme` to `"Tokyo Night"`
- **THEN** both keys are stripped from the imported profile's `settings.json`, while unrelated failures preserve built-in or unrecognized themes.

### Requirement: Extension installation

The system SHALL install active extensions via allowlist, VSIX pins, and single-store `--user-data-dir`, skipping catalog-known `removed`, `compromised`, `superseded`, and `retired` IDs from pre-v2 bundles (and skipping `AI` IDs plus stripping AI settings on `--no-ai`) while hard-failing unknown publishers.

#### Scenario: Extensions list missing
- **WHEN** the bundle omits `extensions` or `extensions.enabled` (or sets it to `null`)
- **THEN** `jq -e -s` schema validation succeeds and the script skips extension installation.

#### Scenario: Extension install failure
- **WHEN** an extension install fails
- **THEN** the script continues with remaining extensions and exits non-zero after theme fallback rewrite.

#### Scenario: Pre-v2 bundle with removed or retired IDs and --no-ai skips cleanly
- **WHEN** a pre-v2 bundle contains `gabrielbb.vscode-lombok` (`removed`), `oderwat.indent-rainbow` (`retired`), `jetmartin.bats` (`superseded`), and `anthropic.claude-code` (`AI`), and imports with `--no-ai`
- **THEN** `import-profile.sh` warns and skips those IDs before active publisher checks, strips AI coexistence keys, and installs the remaining active extensions.

## ADDED Requirements

### Requirement: Canonical-bound TOCTOU-free SHA256SUMS verification

`scripts/import-profile.sh` SHALL resolve `canonical_path` FIRST (40-hop symlink limit), copy that exact canonical file ONCE into a private `0600` `mktemp`, and verify the staged bytes against `SHA256SUMS` in the canonical bundle directory: hard-failing on mismatch before any mutation, requiring a unique digest for managed `exports/` bundles, and warning-and-proceeding on missing/unlisted SUMS for third-party bundles.

#### Scenario: Legacy symlink bundle verifies cleanly and mismatch aborts pre-mutation
- **WHEN** importing `exports/rust-profile-crisp.code-profile` (symlink to `rust.code-profile`) or a tampered bundle whose hash mismatches `SHA256SUMS`
- **THEN** the valid symlink verifies against `rust.code-profile` in `exports/SHA256SUMS` without warning, while a mismatch aborts non-zero before any write.
