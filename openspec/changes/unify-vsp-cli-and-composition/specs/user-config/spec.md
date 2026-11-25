# User Configuration Delta

## Purpose

Let users compose personal profiles without a fork while applying validation and trust rules to profile overrides and extra extensions.

## ADDED Requirements

### Requirement: User configuration source

Vsp SHALL read personal config from `$XDG_CONFIG_HOME/vsp/config.jsonc` (default `~/.config/vsp/config.jsonc`) or from the single explicit `--config PATH` source. Vsp MUST NOT auto-load repo-local `.vsp.toml` or `.vsp.jsonc`.

#### Scenario: Repository-local settings are ignored
- **GIVEN** a valid personal config and unrelated repo-local `.vsp.toml` and `.vsp.jsonc`
- **WHEN** a user runs `vsp list` from that repository
- **THEN** it resolves profiles from the personal config and leaves the repository files untouched.

### Requirement: Version one configuration schema

Vsp SHALL require integer `version: 1`. The optional `defaults` object SHALL accept `ux` (`crisp`, `retina`, `default`), `ai` (`full`, `copilot`, `off`), and boolean `rtkTerminal`. The optional `profiles` map SHALL accept `description`, `extends`, `with`, `without`, `overrides`, and `extraExtensions` per profile name.

#### Scenario: Load a custom profile
- **GIVEN** a version-one profile extending a catalog preset and naming an added pack
- **WHEN** a user runs `vsp show` with the custom name
- **THEN** vsp resolves the profile and exposes its description and composed pack selection.

#### Scenario: Refuse an unsupported schema version
- **WHEN** a config-consuming command reads a file with `version: 2`
- **THEN** it exits `4`.

### Requirement: Personal defaults and CLI precedence

Vsp SHALL apply personal defaults to named and Default profile targets and honor explicit UX and AI selections over those defaults. Personal defaults SHALL NOT change workspace output.

#### Scenario: Use personal defaults only for personal targets
- **GIVEN** a personal config declaring a default UX and AI mode
- **WHEN** a user previews a named profile, that profile with explicit UX and AI flags, and a workspace without those flags
- **THEN** the first plan uses the personal defaults, the second uses the explicit values, and the workspace plan excludes the personal defaults.

### Requirement: Override key shape

Vsp SHALL accept literal dotted setting identifiers and `[language]` blocks as override keys. A language block SHALL contain literal setting identifiers.

#### Scenario: Bare setting key is rejected
- **WHEN** a config-consuming command reads an override with a bare `chat` object key
- **THEN** it exits `4` before applying any changes.

### Requirement: Override safety policy

Vsp SHALL reject personal overrides that match the autonomy deny-list with exit `5`. The bundle unsafe-settings option MUST NOT bypass this validation.

#### Scenario: Denied override is rejected
- **WHEN** a profile sync with `--allow-unsafe-settings` reads an override that enables chat auto-approval
- **THEN** it exits `5` before applying any changes.

### Requirement: Exact extension declarations

Vsp SHALL accept only exact `publisher.name` IDs in `extraExtensions`. It SHALL reject publisher trust fields and wildcard IDs with exit `4`, and compromised or removed entries with exit `5`.

#### Scenario: Wildcard trust is refused
- **WHEN** a config-consuming command reads `extraExtensions` with `publisher.*` or a publisher-wide trust field
- **THEN** it exits `4` without enabling any extension.

#### Scenario: Compromised exact ID is refused
- **WHEN** a config-consuming command reads a compromised exact catalog ID in `extraExtensions`
- **THEN** it exits `5`.

### Requirement: Initialize a personal config

`vsp init` SHALL create a valid version-one starter file and refuse to replace an existing file unless the user requests replacement.

#### Scenario: Initialize twice
- **GIVEN** the default config path is absent
- **WHEN** the user runs `vsp init` twice without requesting replacement
- **THEN** the first run writes the starter and exits `0`, and the second preserves it and exits `5`.
