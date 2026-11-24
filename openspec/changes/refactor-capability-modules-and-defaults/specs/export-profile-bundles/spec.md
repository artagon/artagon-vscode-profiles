# Delta Specification: Export Profile Bundles

## MODIFIED Requirements

### Requirement: Export bundle per profile

The system SHALL acquire an ownership-checked export lock, strip all 14 symmetric UX keys dynamically from `modules/ux/crisp/settings.profile.jsonc`, stage and verify all bundles and `SHA256SUMS.tmp` before atomically publishing `exports/<profile>.code-profile` (`0644`), refresh legacy symlinks via the single authoritative `list_legacy_bundle_links()` helper (`ai-profile-*` -> `ai-copilot.code-profile`), and publish sorted `exports/SHA256SUMS` (`0644`).

#### Scenario: Export includes settings and extensions
- **WHEN** export runs for `rust` and both `_merged/rust.json` and `profiles/rust/extensions.json` exist
- **THEN** `exports/rust.code-profile` (`0644`) contains UX-stripped `settings` from `_merged/rust.json` (omitting all 14 UX keys including `editor.fontFamily`, `editor.fontSize`, `workbench.colorTheme`, `workbench.iconTheme`)
- **AND** `extensions.enabled` lists the extension IDs from `profiles/rust/extensions.json` and matches its digest in `exports/SHA256SUMS` (`0644`).

#### Scenario: Legacy ai-profile symlinks target ai-copilot across export and refresh-symlinks
- **WHEN** `scripts/export-profiles.sh` or `scripts/migrate-catalog.sh --refresh-symlinks` refreshes legacy symlinks
- **THEN** `exports/ai-profile-crisp.code-profile` and `exports/ai-profile-retina.code-profile` both symlink to `ai-copilot.code-profile`.
