# Profile Composition Delta

## Purpose

Define how named profiles and workspaces compose from packs and how users obtain portable bundles without a checkout's build outputs.

## ADDED Requirements

### Requirement: Profile sync from the built-in catalog

Vsp SHALL resolve canonical named profiles from its built-in catalog when no external catalog is selected. Canonical profile sync SHALL operate without a prebuilt bundle.

#### Scenario: Synchronize Rust from a clean checkout
- **GIVEN** a valid VS Code store and no `exports/` directory
- **WHEN** a user runs `vsp sync -p rust` and the required VS Code operations succeed
- **THEN** Rust profile sync succeeds using the built-in recipe.

### Requirement: Pack dependencies and conflicts

Vsp SHALL resolve dependencies of selected packs, support explicit additions and exclusions, and refuse conflicting compositions before applying any target changes.

#### Scenario: Change a formatter stack
- **WHEN** a user previews an Astro workspace with `--preset astro --with biome --without ts-eslint-prettier`
- **THEN** the plan includes Biome and excludes the ESLint and Prettier pack.

#### Scenario: Select incompatible formatter packs
- **WHEN** a user selects both conflicting formatter packs without excluding either
- **THEN** vsp exits `4` and does not apply the composition.

### Requirement: Preserve unrelated JSONC settings

When synchronizing a valid JSONC settings file, vsp SHALL preserve unrelated user-owned entries and comments on untouched settings.

#### Scenario: Repeated workspace sync preserves authored settings
- **GIVEN** a workspace settings file with a user-owned setting and an adjacent comment
- **WHEN** vsp sync updates a separate managed setting, and the user repeats the same sync
- **THEN** the managed value is current, and the user-owned entry and its comment remain.

### Requirement: Generate bundles on demand

`vsp export` SHALL write the requested profile bundles and their `SHA256SUMS` to the chosen output directory. `vsp dev dist` SHALL emit the 15 canonical bundles, 22 legacy aliases, and a deterministic checksum manifest.

#### Scenario: Export before importing
- **GIVEN** a clean checkout without generated bundles
- **WHEN** a user runs `vsp export rust --out ./bundles`
- **THEN** `./bundles/rust.code-profile` and `./bundles/SHA256SUMS` are available for import and integrity verification.

#### Scenario: Preserve the Copilot-only aliases
- **WHEN** a user runs `vsp dev dist` twice with the same inputs into separate directories
- **THEN** the output trees and checksums match, and both `ai-profile-crisp.code-profile` and `ai-profile-retina.code-profile` match `ai-copilot.code-profile`.
